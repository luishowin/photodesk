//! The nodes of §6.2's typed edit graph, and the key that gives them identity.
//!
//! ## The key is a Merkle hash, and three things fall out of that
//!
//! A node's key is `H(what it does, the keys of its inputs)`. Because the inputs are
//! themselves keyed, the key covers the whole subgraph beneath it — so two nodes with
//! the same key compute the same image, wherever they sit.
//!
//! §6.2 lists "identity-node elimination, common-subexpression caching, dirty-subgraph
//! invalidation" as benefits that arrive *for free*. They arrive for free from this
//! and from nothing else:
//!
//! - **Common subexpressions** dedupe on insertion: a node whose key is already in the
//!   graph is not added again, so two layers sharing a mask share the subgraph that
//!   computes it.
//! - **Dirty tracking** is key-set subtraction. Recompiling after an edit gives
//!   identical keys for everything the edit did not reach, so the nodes to recompute
//!   are exactly the ones whose keys are new — which handles reordering, insertion and
//!   deletion, not only a slider drag.
//! - **Identity elimination** is not a key property; it happens in `compile`.
//!
//! ## The key is deliberately scale-free
//!
//! Nothing here mentions resolution, because §12.2 renders *the same document* at
//! proxy and at full-res and asserts they match — a claim that only means something if
//! both runs execute one graph. Resolution belongs to execution.
//!
//! It does belong to *caching*, though, and §9.3 says why in the strongest terms: a
//! proxy-resolution mask silently serving a full-resolution export is "a soft edge
//! nobody ordered, appearing only in the exported file, which is the worst place to
//! find it". So [`Node::cache_key`] folds the resolution in, and the two keys are
//! separate on purpose — §9.3's own rule is "different caches, different keys, don't
//! share one scheme".

use serde::Serialize;

use crate::photodesk::document::{ColorSpace, Geometry, MaskComponent, MaskOp, Params, Stroke};

/// A node's position in [`Graph::nodes`]. Always less than the index of any node that
/// consumes it — the graph is built in topological order and stays that way.
///
/// [`Graph::nodes`]: super::Graph::nodes
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "graph.ts"))]
pub struct NodeId(pub usize);

/// `H(what it does, the keys of its inputs)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "String")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "graph.ts", type = "string"))]
pub struct NodeKey([u8; 32]);

impl NodeKey {
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl From<NodeKey> for String {
    fn from(k: NodeKey) -> String {
        k.to_hex()
    }
}

impl std::fmt::Debug for NodeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Twelve hex digits is enough to tell nodes apart in a failure message and
        // short enough that a whole graph fits on a screen.
        write!(f, "{}", &self.to_hex()[..12])
    }
}

/// The expensive half of a mask component: everything §9.3 says belongs in a cache key.
///
/// Feather and invert are **not** here, and their absence is the point. §9.3:
///
/// > Feather is deliberately not in the key. Feather is a cheap blur applied to the
/// > cached bitmap on the way out; putting it in `input_state_hash` would make every
/// > nudge of the feather slider invalidate the embedding and re-run the segmenter,
/// > turning a free control into a multi-second one.
///
/// Splitting them into separate nodes turns that from a rule someone has to remember
/// into a shape that cannot express the mistake: a feather node sits downstream of the
/// shape node, so the shape's key cannot contain the feather radius.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "graph.ts"))]
pub enum MaskShape {
    Brush { strokes: Vec<Stroke> },
    Linear { from: [f32; 2], to: [f32; 2] },
    Radial { center: [f32; 2], radius: [f32; 2] },
    Luminance { range: [f32; 2] },
    Color { target: [f32; 3], tolerance: f32 },
    Ai { kind: String },
}

impl MaskShape {
    /// §8: `luminance` and `color` "are the only components computed *from pixels*",
    /// and the frozen register item says which pixels — **the layer's input**.
    ///
    /// This is the function that makes that structural. A component that reads pixels
    /// takes the layer's input as a graph input, so it is keyed by everything upstream
    /// of it; one that does not takes no input at all and is keyed only by its own
    /// parameters. The consequence is worth stating because it is easy to get
    /// backwards: two layers with the *same linear gradient* share one node, and two
    /// layers with the *same luminance range* at different stack positions do not —
    /// correctly, because they select different pixels.
    pub fn reads_pixels(&self) -> bool {
        matches!(self, MaskShape::Luminance { .. } | MaskShape::Color { .. })
    }

    /// Split a document component into the part that is expensive and the parts that
    /// are applied to its result.
    pub fn split(component: &MaskComponent) -> (MaskShape, Option<f32>, bool) {
        match component {
            MaskComponent::Brush { strokes, feather, invert } => (
                MaskShape::Brush { strokes: strokes.clone() },
                *feather,
                invert.unwrap_or(false),
            ),
            MaskComponent::Linear { from, to, feather, invert } => (
                MaskShape::Linear { from: *from, to: *to },
                *feather,
                invert.unwrap_or(false),
            ),
            MaskComponent::Radial { center, radius, feather, invert } => (
                MaskShape::Radial { center: *center, radius: *radius },
                *feather,
                invert.unwrap_or(false),
            ),
            MaskComponent::Luminance { range, feather, invert } => (
                MaskShape::Luminance { range: *range },
                *feather,
                invert.unwrap_or(false),
            ),
            MaskComponent::Color { target, tolerance, feather, invert } => (
                MaskShape::Color { target: *target, tolerance: *tolerance },
                *feather,
                invert.unwrap_or(false),
            ),
            MaskComponent::Ai { kind, feather, invert } => (
                MaskShape::Ai { kind: kind.clone() },
                *feather,
                invert.unwrap_or(false),
            ),
        }
    }
}

/// What a node does.
///
/// The granularity is §7.3's, not §5's. §5 lists fourteen stages; §7.3 requires stages
/// 2–9 **fused into a single pass** — "fifteen discrete render passes at 2 MP f16 means
/// roughly half a gigabyte of traffic, which no amount of ALU saves you from" — so an
/// `adjust` layer is one node rather than eight. The graph's job is the order *between*
/// layers; the order *within* the fused pass is the shader's, and §5 freezes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "graph.ts"))]
pub enum NodeKind {
    /// §5 stage 0 — the decoded source, normalised into the working space.
    ///
    /// No parameters: the source is identified by the document's `source.hash`, which
    /// is folded into the graph's root key rather than into this node, so that two
    /// documents for the same photograph share nothing accidentally and everything
    /// deliberately.
    Source,
    /// §5 stage 1 — perspective, rotate, crop. Once for the whole image, not per layer.
    Geometry(Geometry),
    /// §5 stages 2–9 for one layer, fused (§7.3).
    Adjust(Params),
    /// §8's expensive part. See [`MaskShape`].
    MaskShape(MaskShape),
    /// A blur on an already-computed mask. Separate from the shape so §9.3's
    /// feather-is-not-in-the-key rule is a shape rather than a convention.
    MaskFeather { radius: f32 },
    /// `1 - x` on an already-computed mask. Cheap and post, so it is out of the key by
    /// §9.3's same argument.
    MaskInvert,
    /// §8's composition, folded over the components in declared order.
    ///
    /// A struct variant rather than a newtype one, and not for style: serde's
    /// internally-tagged representation cannot serialise a newtype variant holding a
    /// string, and `MaskOp` is a string. `NodeKind(MaskOp)` compiles, generates
    /// plausible TypeScript, and fails at run time the first time a two-component
    /// mask is keyed.
    MaskCompose { mode: MaskOp },
    /// §5's per-layer step: "result composited over the layer's input, through its
    /// mask, at its opacity".
    ///
    /// Inputs are `[layer_input, adjusted, mask?]` in that order. A layer with no mask
    /// at full opacity gets no such node at all — §5 says its "mask multiply and
    /// composite are identity and both are skipped".
    Composite { opacity: f32 },
    /// §5 stage 13 — display or export encode, carrying §16 #11's gamut policy.
    Encode { colorspace: ColorSpace },
}

impl NodeKind {
    /// A short tag for failure messages and for the key's domain separation.
    pub fn tag(&self) -> &'static str {
        match self {
            NodeKind::Source => "source",
            NodeKind::Geometry(_) => "geometry",
            NodeKind::Adjust(_) => "adjust",
            NodeKind::MaskShape(_) => "mask_shape",
            NodeKind::MaskFeather { .. } => "mask_feather",
            NodeKind::MaskInvert => "mask_invert",
            NodeKind::MaskCompose { .. } => "mask_compose",
            NodeKind::Composite { .. } => "composite",
            NodeKind::Encode { .. } => "encode",
        }
    }
}

/// One node of the compiled graph.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "graph.ts"))]
pub struct Node {
    pub kind: NodeKind,
    /// Always lower than this node's own index. The graph is built in topological
    /// order and there is no edge that can break it, because a node is only ever
    /// constructed from nodes that already exist.
    pub inputs: Vec<NodeId>,
    pub key: NodeKey,
    /// Which document layer this came from, if any, so the UI can say *which* layer is
    /// re-rendering without the graph having to know what a UI is.
    ///
    /// This is the graph's whole concession to the front end, and §6.2 is explicit
    /// that there is no more than that: "The graph has **no UI**. Ever."
    pub layer: Option<String>,
}

impl Node {
    /// The cache key for this node's result at a given resolution.
    ///
    /// Separate from [`Node::key`] because §9.3 requires it: `mask_resolution` is in
    /// the mask key precisely so "proxy and export are separate entries and the miss
    /// is explicit". A single scheme covering both would either put resolution into
    /// the graph — breaking §12.2's premise that one graph runs at both scales — or
    /// leave it out of the cache, which is the failure §9.3 spends a paragraph on.
    pub fn cache_key(&self, width: u32, height: u32) -> NodeKey {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"photodesk.cache.v1");
        hasher.update(&self.key.0);
        hasher.update(&width.to_le_bytes());
        hasher.update(&height.to_le_bytes());
        NodeKey(*hasher.finalize().as_bytes())
    }
}

/// Builds [`NodeKey`]s. Everything that changes the pixels a node produces goes in;
/// nothing else does.
pub(super) struct KeyBuilder(blake3::Hasher);

impl KeyBuilder {
    pub(super) fn new(kind: &NodeKind, inputs: &[NodeKey]) -> Self {
        let mut hasher = blake3::Hasher::new();
        // Domain separation, so a future key scheme cannot collide with this one and
        // serve a stale cache entry that hashes the same by coincidence.
        hasher.update(b"photodesk.node.v1");
        hasher.update(kind.tag().as_bytes());
        hasher.update(&[0xff]);
        // The parameters, via their serialised form. Using serde rather than a
        // hand-written encoder means a field added to a node kind is covered by the
        // key automatically — the alternative is a match arm somebody forgets to
        // extend, and the symptom of that is a stale render nobody can reproduce.
        let json = serde_json::to_vec(kind).expect("node kinds serialise");
        hasher.update(&json);
        hasher.update(&[0xff]);
        for input in inputs {
            hasher.update(&input.0);
        }
        KeyBuilder(hasher)
    }

    /// Fold in the root context: the photograph and the pipeline ordering.
    ///
    /// The source hash belongs here rather than in a `Source` node's parameters
    /// because it has to reach *every* key — two documents for different photographs
    /// must not share a cache entry for an identically-parameterised adjustment. Same
    /// for `pipeline_version`: §5 freezes that order is versioned, and a graph
    /// compiled under one ordering must not serve a cache entry made under another.
    pub(super) fn with_root(mut self, source_hash: &str, pipeline_version: u32) -> Self {
        self.0.update(source_hash.as_bytes());
        self.0.update(&pipeline_version.to_le_bytes());
        self
    }

    pub(super) fn finish(self) -> NodeKey {
        NodeKey(*self.0.finalize().as_bytes())
    }
}
