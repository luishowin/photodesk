//! Spike C, part 1 — does one WGSL source lower to GLSL ES 3.00?
//!
//! §2.3 says this outcome "decides how every shader in the project is written, and
//! that is not a decision to discover twenty shaders in". So the shader under test is
//! a realistic fused per-pixel pass rather than a toy: a large uniform block with
//! fixed-size arrays in it, dynamic indexing, a data-dependent loop bound, and a
//! switch. If naga only handles the easy half, that is worth finding out now.

use naga::ShaderStage;
use photodesk_renderer_spike::{ADJUST_WGSL, to_glsl_es300};

#[test]
fn fragment_stage_lowers_to_glsl_es_300() {
    match to_glsl_es300(ADJUST_WGSL, "fs_main", ShaderStage::Fragment) {
        Ok(t) => {
            println!("fragment stage: lowered, {} bytes of GLSL", t.source.len());
            println!("bound resources: {:?}", t.uniform_names);
            assert!(
                t.source.contains("#version 300 es"),
                "backend did not emit an ES 3.00 header; got:\n{}",
                &t.source[..t.source.len().min(200)]
            );
            // The constructs GLSL ES 3.0 does not have must not appear in the output.
            for forbidden in ["layout(std430", "imageStore", "buffer "] {
                assert!(
                    !t.source.contains(forbidden),
                    "output contains `{forbidden}`, which GLSL ES 3.00 has no notion of"
                );
            }
            println!("\n--- first 60 lines ---");
            for line in t.source.lines().take(60) {
                println!("{line}");
            }
        }
        Err(e) => panic!(
            "§7.2's transpilation candidate is dead for a realistic pass.\n{e}\n\
             That is a finding, not a build failure: §2.3's hand-written-GLSL fallback \
             becomes the path and the shader body has to be the shared artefact."
        ),
    }
}

#[test]
fn vertex_stage_lowers_to_glsl_es_300() {
    let t = to_glsl_es300(ADJUST_WGSL, "vs_main", ShaderStage::Vertex)
        .unwrap_or_else(|e| panic!("vertex stage failed to lower:\n{e}"));
    println!("vertex stage: lowered, {} bytes of GLSL", t.source.len());
    assert!(t.source.contains("#version 300 es"));
    // The full-screen triangle derives its positions from gl_VertexID, so a vertex
    // buffer is one fewer resource to bind and one fewer thing to get wrong.
    assert!(
        t.source.contains("gl_VertexID"),
        "vertex_index did not survive as gl_VertexID; the no-vertex-buffer trick is \
         what keeps the preview path's binding surface small"
    );
}

/// The negative control. §2.3's whole argument is that *compute* is what has no GLSL
/// ES 3.0 target — not WGSL in general. If a compute entry point also lowered
/// cleanly, the reasoning behind authoring fragment-first would be wrong, and the
/// fragment result above would be a coincidence rather than a consequence.
#[test]
fn compute_stage_does_not_lower_which_is_the_whole_point() {
    const COMPUTE: &str = r#"
        struct Params { gain: f32 }
        @group(0) @binding(0) var<storage, read> params: Params;
        @group(0) @binding(1) var src: texture_2d<f32>;
        @group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;

        @compute @workgroup_size(8, 8, 1)
        fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            let c = textureLoad(src, id.xy, 0);
            textureStore(dst, id.xy, c * params.gain);
        }
    "#;

    let result = to_glsl_es300(COMPUTE, "main", ShaderStage::Compute);
    match result {
        Err(e) => {
            println!(
                "compute stage refused, as expected — this is the shape RapidRAW's \
                 entire chain has (FORK-AUDIT.md):\n{e}"
            );
        }
        Ok(t) => {
            // Not a hard failure: if a future naga can lower this, the finding changes
            // and the register should hear about it rather than the test hiding it.
            println!(
                "NOTE: naga lowered a compute shader to {} bytes. §2.3's reasoning \
                 assumed it could not. Re-read the output before trusting it — \
                 GLSL ES 3.00 has no compute stage, so this is more likely a backend \
                 that ignored the stage than a target that gained one.",
                t.source.len()
            );
            assert!(
                !t.source.contains("#version 300 es") || !t.source.contains("main("),
                "naga emitted something claiming to be ES 3.00 compute; that is not a \
                 thing WebGL2 can run, and the register entry needs revisiting"
            );
        }
    }
}

/// §2.3 claims a per-layer uniform block fits comfortably where RapidRAW's 32-slot
/// storage buffer would not. That claim is in the spec; this checks it.
#[test]
fn uniform_block_fits_the_gles3_guaranteed_minimum() {
    let t = to_glsl_es300(ADJUST_WGSL, "fs_main", ShaderStage::Fragment)
        .expect("fragment stage should lower");

    // Every uniform-block member naga emitted, in declaration order.
    let block = t
        .source
        .split("uniform ")
        .find(|s| s.contains("Adjustments") || s.contains("adj"))
        .unwrap_or("");
    let floats = block.matches("float ").count()
        + block.matches("vec4 ").count() * 4
        + block.matches("uvec4 ").count() * 4;
    let arrays: usize = block
        .match_indices("[8]")
        .count();
    // Arrays are 8 × vec4 each.
    let approx_bytes = (floats + arrays * 8 * 4) * 4;

    const GLES3_MIN_UBO_BYTES: usize = 16 * 1024;
    println!(
        "uniform block: ~{approx_bytes} bytes against GLES3's guaranteed minimum of \
         {GLES3_MIN_UBO_BYTES}"
    );
    assert!(
        approx_bytes < GLES3_MIN_UBO_BYTES,
        "the per-layer block does not fit a guaranteed UBO ({approx_bytes} bytes); \
         §2.3's argument that §5's per-layer loop is what makes a uniform buffer \
         sufficient would need revisiting"
    );
}

/// Writes the lowered GLSL where the WebGL2 harness can load it, so the browser runs
/// the *transpiled* shader rather than a hand-written twin of it. A twin would make
/// the whole exercise circular: §0's one-shader-source invariant is only tested if
/// the thing that runs is the thing that was generated.
#[test]
fn emit_glsl_for_the_web_harness() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/web/generated");
    std::fs::create_dir_all(out_dir).expect("create web/generated");

    for (entry, stage, name) in [
        ("vs_main", ShaderStage::Vertex, "adjust.vert"),
        ("fs_main", ShaderStage::Fragment, "adjust.frag"),
    ] {
        let t = to_glsl_es300(ADJUST_WGSL, entry, stage)
            .unwrap_or_else(|e| panic!("{entry} failed to lower:\n{e}"));
        let path = format!("{out_dir}/{name}");
        std::fs::write(&path, &t.source).expect("write generated shader");
        println!("wrote {path} ({} bytes)", t.source.len());
    }
}
