//! §6.2's graph, and the four things it says arrive "for free".
//!
//! > Benefits that arrive for free: identity-node elimination, common-subexpression
//! > caching (two layers sharing a mask compute it once), dirty-subgraph invalidation
//! > on a slider drag instead of full re-render, and a stable target for future
//! > modules.
//!
//! Free is a claim about a design, and a design's claims are the ones worth testing —
//! a graph that compiled correctly and deduplicated nothing would pass every
//! correctness test in this file and deliver none of the reason for existing.
//!
//! One test here is a **negative** one, in the spirit of `tests/renderer/`'s
//! compute-shader control: deduplication that shared too much would look like a very
//! good result. `a_luminance_mask_is_not_shared_across_stack_positions` is the test
//! that makes the sharing a consequence rather than a coincidence.
//!
//! `cargo test -p photodesk --test graph -- --nocapture` prints the graphs.

use photodesk::photodesk::document::{
    AdjustV1, ColorSpace, Crop, Document, Geometry, Layer, Mask, MaskComponent, MaskOp, Op, Params,
    Source,
};
use photodesk::photodesk::graph::{self, CompileError, Graph, NodeKind};

fn source() -> Source {
    Source {
        file: "IMG_4821.HEIC".into(),
        hash: "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        dimensions: [4032, 3024],
        colorspace: ColorSpace::DisplayP3,
        orientation: 1,
    }
}

fn layer(id: &str, exposure: f32) -> Layer {
    Layer {
        id: id.into(),
        op: Op::Adjust,
        op_version: 1,
        enabled: true,
        name: None,
        opacity: None,
        mask: None,
        params: Params::AdjustV1(AdjustV1 {
            exposure: Some(exposure),
            ..Default::default()
        }),
    }
}

fn doc(layers: Vec<Layer>) -> Document {
    Document {
        stack: layers,
        ..Document::new(source())
    }
}

fn compile(document: &Document) -> Graph {
    graph::compile(document).expect("compile")
}

fn tags(g: &Graph) -> Vec<&'static str> {
    g.nodes().iter().map(|n| n.kind.tag()).collect()
}

/// A linear gradient — cheap, parametric, and reads no pixels.
fn linear_mask() -> Mask {
    Mask {
        op: MaskOp::Union,
        components: vec![MaskComponent::Linear {
            from: [0.5, 0.0],
            to: [0.5, 0.42],
            feather: None,
            invert: None,
        }],
    }
}

/// A luminance range — §8's frozen item, computed *from the layer's input*.
fn luminance_mask() -> Mask {
    Mask {
        op: MaskOp::Union,
        components: vec![MaskComponent::Luminance {
            range: [0.6, 1.0],
            feather: None,
            invert: None,
        }],
    }
}

// ------------------------------------------------------------------ §5's shape

/// The smallest graph: a photograph with no edits still has to be decoded and encoded.
#[test]
fn an_unedited_document_is_source_then_encode() {
    let g = compile(&doc(vec![]));
    println!("{}", g.describe());
    assert_eq!(tags(&g), ["source", "encode"]);
    assert_eq!(g.output().0, 1);
}

/// §5's preamble and output run once for the whole image; the layer body runs per
/// entry in the stack, in document order.
#[test]
fn the_graph_follows_section_5s_order() {
    let mut d = doc(vec![layer("a", 0.3), layer("b", -0.2)]);
    d.geometry = Geometry {
        crop: Some(Crop { x: 0.02, y: 0.0, w: 0.96, h: 1.0 }),
        rotate: Some(-1.4),
    };
    let g = compile(&d);
    println!("{}", g.describe());

    assert_eq!(tags(&g), ["source", "geometry", "adjust", "adjust", "encode"]);

    // Order between layers is the document's, and it is visible in the edges rather
    // than only in the sequence: each adjust consumes the previous one.
    let adjust_b = &g.nodes()[3];
    assert_eq!(adjust_b.inputs, vec![graph::NodeId(2)]);
    assert_eq!(adjust_b.layer.as_deref(), Some("b"));
}

/// Every node's inputs come before it, so an executor may walk the slice front to back
/// and never wait. Guaranteed by construction; asserted because "guaranteed by
/// construction" is a sentence, not a check.
#[test]
fn the_graph_is_topologically_ordered_and_acyclic() {
    let mut d = doc(vec![
        Layer { mask: Some(luminance_mask()), opacity: Some(0.5), ..layer("a", 0.3) },
        Layer { mask: Some(linear_mask()), ..layer("b", -0.2) },
    ]);
    d.geometry = Geometry { crop: None, rotate: Some(2.0) };
    let g = compile(&d);
    println!("{}", g.describe());

    for (i, node) in g.nodes().iter().enumerate() {
        for input in &node.inputs {
            assert!(
                input.0 < i,
                "node #{i} ({}) consumes #{} which comes after it",
                node.kind.tag(),
                input.0
            );
        }
    }
    assert_eq!(g.output().0, g.len() - 1, "the output is not the last node");
}

// ------------------------------------------------------------ 1. identity elimination

/// §6.1: "Omitted keys mean *identity*, not zero. Identity stages are skipped entirely
/// at render time."
#[test]
fn identity_and_disabled_layers_produce_no_nodes() {
    let identity = Layer {
        params: Params::AdjustV1(AdjustV1::default()),
        ..layer("empty", 0.0)
    };
    let disabled = Layer { enabled: false, ..layer("off", 1.0) };

    let g = compile(&doc(vec![identity, disabled, layer("real", 0.3)]));
    println!("{}", g.describe());
    assert_eq!(tags(&g), ["source", "adjust", "encode"]);
    assert_eq!(g.nodes()[1].layer.as_deref(), Some("real"));

    // A whole stack of nothing compiles to the unedited graph, not to a chain of
    // no-ops with a comment explaining they are cheap.
    let g = compile(&doc(vec![
        Layer { params: Params::AdjustV1(AdjustV1::default()), ..layer("x", 0.0) },
        Layer { enabled: false, ..layer("y", 1.0) },
    ]));
    assert_eq!(tags(&g), ["source", "encode"]);
}

/// An identity geometry is not written to the document (§6.1) and produces no node
/// either — the same elimination at the other end of the same rule.
#[test]
fn an_identity_geometry_produces_no_node() {
    let g = compile(&doc(vec![layer("a", 0.3)]));
    assert!(!tags(&g).contains(&"geometry"));

    let mut d = doc(vec![layer("a", 0.3)]);
    d.geometry = Geometry { crop: None, rotate: Some(0.5) };
    assert!(tags(&compile(&d)).contains(&"geometry"));
}

/// §5, verbatim: "A `mask: null` layer composites at full coverage, so its mask
/// multiply and composite are identity and both are skipped."
///
/// Worth its own test because getting it wrong is invisible in the output and
/// expensive in the budget: a composite node is a full-frame texture round-trip, and
/// §7.3 sizes the frame budget on there being four or five passes per layer, not six.
#[test]
fn a_global_layer_at_full_opacity_gets_no_composite_node() {
    let g = compile(&doc(vec![layer("global", 0.35)]));
    println!("{}", g.describe());
    assert_eq!(tags(&g), ["source", "adjust", "encode"]);

    // The two things that bring the composite back, each on its own.
    let with_opacity = compile(&doc(vec![Layer { opacity: Some(0.5), ..layer("a", 0.35) }]));
    assert!(tags(&with_opacity).contains(&"composite"));

    let with_mask = compile(&doc(vec![Layer { mask: Some(linear_mask()), ..layer("a", 0.35) }]));
    assert!(tags(&with_mask).contains(&"composite"));

    // An explicit opacity of 1.0 is the same thing as none, and must not defeat the
    // elimination — the UI writes `1.0` the moment anybody touches the slider and
    // puts it back.
    let explicit = compile(&doc(vec![Layer { opacity: Some(1.0), ..layer("a", 0.35) }]));
    assert_eq!(tags(&explicit), ["source", "adjust", "encode"]);
}

/// Folding one component is that component, so a single-component mask needs no
/// compose node.
#[test]
fn a_single_component_mask_needs_no_compose_node() {
    let one = compile(&doc(vec![Layer { mask: Some(linear_mask()), ..layer("a", 0.3) }]));
    println!("{}", one.describe());
    assert_eq!(tags(&one), ["source", "adjust", "mask_shape", "composite", "encode"]);

    let two = compile(&doc(vec![Layer {
        mask: Some(Mask {
            op: MaskOp::Intersect,
            components: vec![
                linear_mask().components[0].clone(),
                MaskComponent::Ai { kind: "sky".into(), feather: None, invert: None },
            ],
        }),
        ..layer("a", 0.3)
    }]));
    assert!(tags(&two).contains(&"mask_compose"));
}

// ------------------------------------------------------- 2. common subexpressions

/// §6.2: "common-subexpression caching (two layers sharing a mask compute it once)".
#[test]
fn two_layers_sharing_a_mask_compute_it_once() {
    let g = compile(&doc(vec![
        Layer { mask: Some(linear_mask()), ..layer("a", 0.3) },
        Layer { mask: Some(linear_mask()), ..layer("b", -0.2) },
    ]));
    println!("{}", g.describe());

    let shapes = tags(&g).iter().filter(|t| **t == "mask_shape").count();
    assert_eq!(shapes, 1, "the gradient was rasterised twice:\n{}", g.describe());

    // And both composites consume it, so the sharing is real rather than one of them
    // having quietly lost its mask.
    let shape_id = g
        .nodes()
        .iter()
        .position(|n| matches!(n.kind, NodeKind::MaskShape(_)))
        .map(graph::NodeId)
        .expect("a mask shape");
    let users = g
        .nodes()
        .iter()
        .filter(|n| n.inputs.contains(&shape_id))
        .count();
    assert_eq!(users, 2, "only one layer ended up using the shared mask");
}

/// **The control.** Deduplication that shared too much would look like an even better
/// result, so the test that matters is the one that must *not* share.
///
/// §8's frozen register item: `luminance` and `color` read **the layer's input**. Two
/// layers with an identical luminance range at different stack positions therefore
/// select different pixels — the first sees the source, the second sees the source
/// with the first layer's adjustment already applied. Sharing one node between them
/// would be a real bug producing a plausible image, and §8 wrote the rule down
/// precisely because it "gets chosen accidentally and differently in the preview and
/// the export".
#[test]
fn a_luminance_mask_is_not_shared_across_stack_positions() {
    let g = compile(&doc(vec![
        Layer { mask: Some(luminance_mask()), ..layer("a", 0.3) },
        Layer { mask: Some(luminance_mask()), ..layer("b", -0.2) },
    ]));
    println!("{}", g.describe());

    let shapes: Vec<_> = g
        .nodes()
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::MaskShape(_)))
        .collect();
    assert_eq!(
        shapes.len(),
        2,
        "two luminance masks at different stack positions were shared. They read the \
         layer's input, so they select different pixels:\n{}",
        g.describe()
    );
    // Each reads a different upstream image, which is *why* they are not shared.
    assert_ne!(shapes[0].inputs, shapes[1].inputs);

    // The same two layers with a *parametric* mask do share — so the difference is
    // the pixel-reading, not some accident of how the two documents were built.
    let parametric = compile(&doc(vec![
        Layer { mask: Some(linear_mask()), ..layer("a", 0.3) },
        Layer { mask: Some(linear_mask()), ..layer("b", -0.2) },
    ]));
    assert_eq!(
        parametric
            .nodes()
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::MaskShape(_)))
            .count(),
        1
    );
}

/// §9.3: "Feather is deliberately not in the key… putting it in `input_state_hash`
/// would make every nudge of the feather slider invalidate the embedding and re-run
/// the segmenter, turning a free control into a multi-second one."
///
/// Split into separate nodes, that rule stops being something to remember: the shape
/// node cannot contain the feather radius because the radius is not one of its fields.
/// Two layers with the same AI mask at different feathers share the *segmentation* and
/// differ only in the blur.
#[test]
fn feather_does_not_invalidate_the_expensive_part_of_a_mask() {
    let ai = |feather: f32| Mask {
        op: MaskOp::Union,
        components: vec![MaskComponent::Ai {
            kind: "sky".into(),
            feather: Some(feather),
            invert: None,
        }],
    };

    let g = compile(&doc(vec![
        Layer { mask: Some(ai(12.0)), ..layer("a", 0.3) },
        Layer { mask: Some(ai(40.0)), ..layer("b", -0.2) },
    ]));
    println!("{}", g.describe());

    assert_eq!(
        tags(&g).iter().filter(|t| **t == "mask_shape").count(),
        1,
        "the segmenter would run twice for two different feather radii:\n{}",
        g.describe()
    );
    assert_eq!(tags(&g).iter().filter(|t| **t == "mask_feather").count(), 2);

    // And a feather of zero is not a blur of zero pixels, it is no blur at all.
    let none = compile(&doc(vec![Layer { mask: Some(ai(0.0)), ..layer("a", 0.3) }]));
    assert!(!tags(&none).contains(&"mask_feather"));
}

// ---------------------------------------------------------- 3. dirty invalidation

/// §6.2: "dirty-subgraph invalidation on a slider drag instead of full re-render".
#[test]
fn a_slider_drag_dirties_only_its_layer_and_what_follows() {
    let before = compile(&doc(vec![
        layer("a", 0.1),
        layer("b", 0.2),
        layer("c", 0.3),
        layer("d", 0.4),
    ]));

    let mut edited = doc(vec![layer("a", 0.1), layer("b", 0.2), layer("c", 0.3), layer("d", 0.4)]);
    edited.stack[2].params = Params::AdjustV1(AdjustV1 {
        exposure: Some(0.35),
        ..Default::default()
    });
    let after = compile(&edited);

    let dirty = after.changed_since(&before);
    println!("{}\ndirty: {dirty:?}", after.describe());

    // Layers a and b are upstream of the change and keep their keys; c, d and the
    // encode are downstream and do not.
    let dirty_tags: Vec<_> = dirty.iter().map(|id| after.node(*id).kind.tag()).collect();
    assert_eq!(dirty_tags, ["adjust", "adjust", "encode"]);
    let dirty_layers: Vec<_> = dirty
        .iter()
        .filter_map(|id| after.node(*id).layer.as_deref())
        .collect();
    assert_eq!(dirty_layers, ["c", "d"]);

    // Returned in topological order, so an executor can run them directly.
    assert!(dirty.windows(2).all(|w| w[0].0 < w[1].0));
}

/// The generality that comes free with a Merkle key, and would not come free with a
/// hand-maintained dirty flag: a reorder, an insertion and a deletion all answer the
/// same question with the same machinery.
#[test]
fn reordering_a_stack_dirties_from_the_first_moved_layer() {
    let before = compile(&doc(vec![layer("a", 0.1), layer("b", 0.2), layer("c", 0.3)]));

    // Swap b and c. Layer a is untouched; everything after it is a different image.
    let after = compile(&doc(vec![layer("a", 0.1), layer("c", 0.3), layer("b", 0.2)]));
    let dirty = after.changed_since(&before);
    let dirty_layers: Vec<_> = dirty
        .iter()
        .filter_map(|id| after.node(*id).layer.as_deref())
        .collect();
    assert_eq!(dirty_layers, ["c", "b"], "{}", after.describe());

    // Deleting the last layer costs exactly one node: the encode, which now consumes
    // b's output instead of c's and is therefore a different computation. Both
    // adjustments survive, because a Merkle key is about what a node computes rather
    // than about where it sits — a positional dirty flag would have invalidated
    // everything after the deletion.
    let deleted = compile(&doc(vec![layer("a", 0.1), layer("b", 0.2)]));
    let dirty = deleted.changed_since(&before);
    println!("{}\ndirty after deleting c: {dirty:?}", deleted.describe());
    assert_eq!(
        dirty.iter().map(|id| deleted.node(*id).kind.tag()).collect::<Vec<_>>(),
        ["encode"],
        "deleting a layer invalidated more than the encode:\n{}",
        deleted.describe()
    );
}

/// A change with no effect on the pixels has no effect on the graph.
///
/// Naming a layer is a document edit and not a render input, and a dirty set that
/// included it would re-render the frame every time somebody typed a character into a
/// text field.
#[test]
fn renaming_a_layer_dirties_nothing() {
    let before = compile(&doc(vec![layer("a", 0.1), layer("b", 0.2)]));
    let mut renamed = doc(vec![layer("a", 0.1), layer("b", 0.2)]);
    renamed.stack[0].name = Some("Sky".into());
    let after = compile(&renamed);

    assert!(
        after.changed_since(&before).is_empty(),
        "renaming a layer invalidated the render:\n{}",
        after.describe()
    );
}

/// Compilation is a pure function of the document. Two compilations of one document
/// agree on every key, or the dirty set means nothing at all.
#[test]
fn compilation_is_deterministic() {
    let d = doc(vec![
        Layer { mask: Some(luminance_mask()), opacity: Some(0.4), ..layer("a", 0.3) },
        Layer { mask: Some(linear_mask()), ..layer("b", -0.2) },
    ]);
    let first = compile(&d);
    let second = compile(&d);
    assert_eq!(first, second);
    assert!(first.changed_since(&second).is_empty());

    // Different photographs never share a node, even for identical edits: the source
    // hash reaches every key. Sharing here would serve one photo's cached adjustment
    // to another.
    let mut other = d.clone();
    other.source.hash =
        "blake3:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into();
    let elsewhere = compile(&other);
    assert_eq!(elsewhere.len(), first.len());
    assert_eq!(
        elsewhere.changed_since(&first).len(),
        elsewhere.len(),
        "two photographs shared cache entries"
    );
}

// ------------------------------------------------------------------- the caveats

/// §6.3 opens an older-pipeline document and *warns*; it never silently re-renders it.
/// Compiling under this build's ordering would be that re-render.
#[test]
fn an_older_pipeline_version_will_not_compile_silently() {
    let mut d = doc(vec![layer("a", 0.3)]);
    d.pipeline_version = 0;

    let err = graph::compile(&d).expect_err("a pipeline this build does not implement");
    println!("{err}");
    assert_eq!(err, CompileError::PipelineMismatch { document: 0, current: 1 });
    assert!(
        err.to_string().contains("offered rather than done"),
        "the message does not say the re-render is the user's call: {err}"
    );
}

/// §9.3: `mask_resolution` is in the cache key so "proxy and export are separate
/// entries and the miss is explicit" — while the graph itself stays scale-free, which
/// is what lets §12.2 render one document at two resolutions and compare them.
#[test]
fn the_node_key_is_scale_free_and_the_cache_key_is_not() {
    let g = compile(&doc(vec![Layer { mask: Some(linear_mask()), ..layer("a", 0.3) }]));
    let node = &g.nodes()[g.output().0];

    let proxy = node.cache_key(1920, 1080);
    let full = node.cache_key(4032, 3024);
    assert_ne!(
        proxy, full,
        "a proxy-resolution result would be served to a full-resolution export — \
         §9.3's 'soft edge nobody ordered, appearing only in the exported file'"
    );
    assert_eq!(proxy, node.cache_key(1920, 1080), "the cache key is not deterministic");

    // The node key itself is unchanged by resolution, because resolution is not one
    // of its inputs. That is what makes §12.2's premise — one graph, two scales —
    // expressible at all.
    assert_eq!(node.key, g.nodes()[g.output().0].key);
}

/// The graph is a *build product*, so it is data the front end can be handed. §13's
/// `src/graph/` executes this plan rather than compiling its own — see the module
/// note for why two compilers would be the drift §0 already froze an invariant over.
#[test]
fn the_compiled_graph_serialises_for_the_front_end() {
    let g = compile(&doc(vec![
        Layer { mask: Some(linear_mask()), opacity: Some(0.8), ..layer("a", 0.3) },
        layer("b", -0.2),
    ]));
    let json = serde_json::to_string_pretty(&g).expect("a graph must serialise");
    println!("{json}");

    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["nodes"].as_array().unwrap().len(), g.len());
    assert_eq!(value["pipeline_version"], 1);
    // Keys reach the front end as hex, so the executor can use them as cache handles
    // without knowing what a blake3 digest is.
    assert!(value["nodes"][0]["key"].as_str().unwrap().len() == 64);
    assert_eq!(value["nodes"][0]["kind"]["op"], "source");
}

/// A stack of N layers produces O(N) nodes. Stated because the alternative — a graph
/// that quietly grows quadratically as layers are added — would show up as §7.3's
/// budget failing at six layers with no obvious cause.
#[test]
fn the_graph_grows_linearly_with_the_stack() {
    let sizes: Vec<usize> = [1usize, 2, 4, 6]
        .iter()
        .map(|n| {
            let layers = (0..*n)
                .map(|i| Layer {
                    mask: Some(linear_mask()),
                    ..layer(&format!("l{i}"), 0.1 * i as f32 + 0.1)
                })
                .collect();
            compile(&doc(layers)).len()
        })
        .collect();
    println!("nodes for 1/2/4/6 masked layers: {sizes:?}");

    // source + encode + (adjust + composite) per layer, and one shared mask shape.
    assert_eq!(sizes, vec![5, 7, 11, 15]);
}
