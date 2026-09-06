//! §6.2's typed edit graph.
//!
//! > **Why a graph and not just a stack:** a stack forces linearity onto operations
//! > that aren't linear. A mask is an *input to* an operation, not an operation in
//! > sequence. An AI removal produces a new image source that downstream nodes
//! > consume. A virtual copy branches. A stack fudges all three; a DAG models them.
//!
//! And the discipline that keeps it from becoming a node editor nobody asked for,
//! quoted because it is the part that gets forgotten:
//!
//! > - The graph has **no UI**. Ever. If a topology can't be represented in the stack
//! >   UI, it isn't allowed to exist yet.
//! > - The graph is a *build product* of the document, reconstructed on load. It is
//! >   not the serialisation format.
//! > - When the stack becomes genuinely lossy for a topology worth having, that's a
//! >   `photodesk: 2` schema bump — a deliberate, migrated event, not a drift.
//!
//! ## Compiled once, in Rust, and executed twice
//!
//! §13 puts "DAG compile, dirty tracking" in the TypeScript front end. That cannot be
//! right, and the reason is the one §0 already froze a whole invariant over.
//!
//! The preview runs in the webview (§7.2) and the export runs natively through wgpu.
//! If each compiled its own graph, the preview would render one topology and the
//! export another — and the two would drift exactly as two shader sources would, with
//! the same property that nobody notices until an exported file differs from what was
//! on screen. §12.2 would then be comparing two *compilations* rather than two
//! executions of one plan, and the one-shader-source invariant it exists to enforce
//! would be enforced over a shader while the graph above it went unchecked.
//!
//! So the graph is compiled here, once, and both renderers execute the same plan. That
//! also keeps §12.1's golden images and §12.2's agreement test headless, which is the
//! same argument that put the document schema in Rust. The front end gets the compiled
//! plan as data — the TypeScript types are generated from these declarations, like the
//! document's.
//!
//! `src/graph/` is therefore the plan's *executor*, not its compiler. §13 is corrected.

mod compile;
mod node;

pub use compile::compile;
pub use node::{MaskShape, Node, NodeId, NodeKey, NodeKind};

use serde::Serialize;

/// Why a document could not be compiled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// §6.3 opens a document written under an older pipeline and *warns*; it never
    /// silently re-renders it. Compiling under this build's ordering would be that
    /// re-render, and it would be invisible — the image would just look different
    /// from the last time the user saw it, with nothing to point at.
    ///
    /// The caller's move is to offer the explicit re-render §6.3 describes, which is
    /// a document edit — `pipeline_version` moves to current — and then a recompile.
    PipelineMismatch { document: u32, current: u32 },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::PipelineMismatch { document, current } => write!(
                f,
                "this document was authored against pipeline version {document} and this \
                 build implements {current}. Re-rendering it on the current pipeline may \
                 change how it looks, so it is offered rather than done"
            ),
        }
    }
}

impl std::error::Error for CompileError {}

/// A compiled document: nodes in topological order, and the one that produces the
/// image.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "graph.ts"))]
pub struct Graph {
    nodes: Vec<Node>,
    output: NodeId,
    /// The ordering this graph was compiled under (§5). Carried rather than assumed,
    /// because a cached result from another ordering is a wrong result.
    pipeline_version: u32,
}

impl Graph {
    /// Every node, in an order where each one's inputs come before it.
    ///
    /// Guaranteed by construction rather than by a sort: a node can only be built from
    /// nodes that already exist, so there is no moment at which the order could be
    /// wrong. An executor may therefore walk this slice front to back and never wait.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    /// The node whose result is the image.
    pub fn output(&self) -> NodeId {
        self.output
    }

    pub fn pipeline_version(&self) -> u32 {
        self.pipeline_version
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The nodes this graph has that `previous` did not — which is exactly the set an
    /// executor has to recompute, everything else being available from the cache.
    ///
    /// §6.2 promises "dirty-subgraph invalidation on a slider drag instead of full
    /// re-render". This delivers it, and delivers more than it: because a node's key
    /// covers its whole subgraph, a *reorder* or a *deletion* produces exactly the
    /// same answer with no extra machinery, where a hand-maintained dirty flag would
    /// need a case for each. A slider drag is simply the case where the new keys form
    /// one chain.
    ///
    /// Returned in topological order, so an executor can run the result directly.
    pub fn changed_since(&self, previous: &Graph) -> Vec<NodeId> {
        let known: std::collections::HashSet<NodeKey> =
            previous.nodes.iter().map(|n| n.key).collect();
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !known.contains(&n.key))
            .map(|(i, _)| NodeId(i))
            .collect()
    }

    /// A compact rendering for failure messages and for reading a graph by eye.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        for (i, node) in self.nodes.iter().enumerate() {
            let inputs: Vec<String> = node.inputs.iter().map(|i| format!("#{}", i.0)).collect();
            out.push_str(&format!(
                "#{i:<3} {:<14} {:<28} {:?}{}\n",
                node.kind.tag(),
                if inputs.is_empty() { "—".into() } else { inputs.join(", ") },
                node.key,
                node.layer
                    .as_ref()
                    .map(|l| format!("  layer `{l}`"))
                    .unwrap_or_default()
            ));
        }
        out.push_str(&format!("output #{}\n", self.output.0));
        out
    }
}
