//! Document → graph (§6.2), and the eliminations §5 and §6.2 both require.
//!
//! > The document is stack-shaped on disk and **the UI is always a stack**. Internally
//! > it compiles to a typed, versioned DAG. […] The graph is a *build product* of the
//! > document, reconstructed on load. It is not the serialisation format.
//!
//! So there is no incremental mutation here. An edit recompiles, and the dirty set
//! falls out of comparing keys — which is both simpler and more general than patching
//! a graph in place, because it handles a reorder and a deletion the same way it
//! handles a slider.
//!
//! ## What gets eliminated, and why each one is required rather than clever
//!
//! - **A disabled layer** produces nothing. It is off.
//! - **An identity layer** produces nothing: §6.1 says omitted keys mean identity and
//!   "identity stages are skipped entirely at render time".
//! - **A layer with no mask at full opacity gets no composite node.** §5 says this
//!   outright — "a `mask: null` layer composites at full coverage, so its mask
//!   multiply and composite are identity and both are skipped, the same elimination
//!   §6.2 performs on identity nodes". Getting it wrong would put a full-frame
//!   texture round-trip in front of every global adjustment, which §7.3 has no room
//!   for.
//! - **An identity geometry** produces nothing.
//! - **A single-component mask needs no compose node**, because folding one value is
//!   that value.
//!
//! ## And what is not eliminated
//!
//! Nothing is reordered, merged across layers, or algebraically simplified. §5 freezes
//! that pipeline order is explicit and versioned; an optimiser that decided two
//! adjacent adjustments could be one pass would be changing the order in a way no
//! document records. Elimination removes what does nothing; it never rewrites what
//! does something.

use std::collections::HashMap;

use crate::photodesk::document::{Document, Mask, PIPELINE_VERSION};

use super::node::{KeyBuilder, MaskShape, Node, NodeId, NodeKey, NodeKind};
use super::{CompileError, Graph};

/// Compile a validated document into the graph that renders it.
pub fn compile(document: &Document) -> Result<Graph, CompileError> {
    // §6.3 opens an older-pipeline document and warns rather than re-rendering it.
    // Compiling it under *this* build's ordering would be that silent re-render, and
    // it would be invisible: the image would simply look different from the last time
    // the user saw it, with nothing to point at.
    if document.pipeline_version != PIPELINE_VERSION {
        return Err(CompileError::PipelineMismatch {
            document: document.pipeline_version,
            current: PIPELINE_VERSION,
        });
    }

    let mut builder = Builder {
        nodes: Vec::new(),
        by_key: HashMap::new(),
        source_hash: document.source.hash.clone(),
        pipeline_version: document.pipeline_version,
    };

    // §5, image preamble.
    let mut current = builder.push(NodeKind::Source, &[], None);
    if !document.geometry.is_identity() {
        current = builder.push(NodeKind::Geometry(document.geometry), &[current], None);
    }

    // §5, layer body. "Stages 2–12 are the loop body; the stack is the loop."
    for layer in &document.stack {
        if !layer.enabled || layer.params.is_identity() {
            continue;
        }
        let layer_id = Some(layer.id.clone());
        let adjusted = builder.push(NodeKind::Adjust(layer.params.clone()), &[current], layer_id.clone());

        // §8's components are built against the *layer's input*, which is `current` —
        // the image as it stands after every earlier layer has been composited. The
        // frozen register item names that, and this is the line that obeys it.
        let mask = layer
            .mask
            .as_ref()
            .map(|m| compile_mask(&mut builder, m, current, layer_id.clone()));

        let opacity = layer.opacity.unwrap_or(1.0);
        current = match (mask, opacity) {
            // §5, verbatim: both are identity and both are skipped.
            (None, o) if o == 1.0 => adjusted,
            (mask, opacity) => {
                let mut inputs = vec![current, adjusted];
                inputs.extend(mask);
                builder.push(NodeKind::Composite { opacity }, &inputs, layer_id)
            }
        };
    }

    // §5, output.
    let output = builder.push(
        NodeKind::Encode { colorspace: document.output.colorspace },
        &[current],
        None,
    );

    Ok(Graph {
        nodes: builder.nodes,
        output,
        pipeline_version: document.pipeline_version,
    })
}

/// §8: components composed in declared order, each with independent feather and invert.
///
/// Feather and invert become their own downstream nodes rather than parameters of the
/// shape — see [`MaskShape`], and §9.3 for why it matters that they are not in the
/// expensive node's key.
fn compile_mask(
    builder: &mut Builder,
    mask: &Mask,
    layer_input: NodeId,
    layer: Option<String>,
) -> NodeId {
    let components: Vec<NodeId> = mask
        .components
        .iter()
        .map(|component| {
            let (shape, feather, invert) = MaskShape::split(component);
            // A component that does not read pixels takes no image input, so it keys
            // identically wherever it appears and two layers sharing it share a node.
            // One that does read pixels is keyed by everything upstream, so it does
            // not — which is correct, and is the half that is easy to get backwards.
            let inputs: Vec<NodeId> = if shape.reads_pixels() {
                vec![layer_input]
            } else {
                vec![]
            };
            let mut node = builder.push(NodeKind::MaskShape(shape), &inputs, layer.clone());
            if let Some(radius) = feather
                && radius != 0.0
            {
                node = builder.push(NodeKind::MaskFeather { radius }, &[node], layer.clone());
            }
            if invert {
                node = builder.push(NodeKind::MaskInvert, &[node], layer.clone());
            }
            node
        })
        .collect();

    match components.len() {
        // A mask with no components selects nothing to say, so it is full coverage —
        // the same thing `mask: null` means. Not reachable from a document today
        // (§8's components are a list and an empty one is legal JSON), and cheaper to
        // handle than to forbid.
        0 => layer_input,
        // Folding one value is that value, so the compose node would be identity.
        1 => components[0],
        _ => builder.push(NodeKind::MaskCompose { mode: mask.op }, &components, layer),
    }
}

/// Accumulates nodes, deduplicating by key.
struct Builder {
    nodes: Vec<Node>,
    by_key: HashMap<NodeKey, NodeId>,
    source_hash: String,
    pipeline_version: u32,
}

impl Builder {
    /// Add a node, or return the existing one that already computes the same thing.
    ///
    /// This single line is §6.2's "common-subexpression caching (two layers sharing a
    /// mask compute it once)". It is not a pass over the graph afterwards; there is
    /// never a moment when the duplicate exists.
    fn push(&mut self, kind: NodeKind, inputs: &[NodeId], layer: Option<String>) -> NodeId {
        let input_keys: Vec<NodeKey> = inputs.iter().map(|i| self.nodes[i.0].key).collect();
        let key = KeyBuilder::new(&kind, &input_keys)
            .with_root(&self.source_hash, self.pipeline_version)
            .finish();

        if let Some(existing) = self.by_key.get(&key) {
            return *existing;
        }

        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            kind,
            inputs: inputs.to_vec(),
            key,
            layer,
        });
        self.by_key.insert(key, id);
        id
    }
}
