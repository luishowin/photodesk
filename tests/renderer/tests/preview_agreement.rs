//! §12.2 across the boundary it was written for: the **front end's own** plan executor
//! against wgpu, on the product's shaders.
//!
//! §0 freezes "one shader source, preview and export", and until v0.1's front end there
//! was nothing on the preview side to compare — Spike C measured its stress shader
//! through a hand-written harness page, which proved the *path* and not the product.
//! What runs here is `src/graph/execute.ts` and `src/canvas/gl.ts`, the same modules
//! `main.ts` imports, inside webkit2gtk-4.1, the binding Tauri embeds.
//!
//! This file emits the inputs and the reference; `web/plan.html` executes and compares;
//! `web/run-probe.py --engine webkit2gtk-4.1 --plan` reports. Split that way for the
//! reason the rest of this harness is: the browser half cannot run in `cargo test`, and
//! a Rust-side reimplementation of the executor would be the twin that makes the whole
//! exercise circular.
//!
//! **What a disagreement here would mean**, in the order worth checking:
//!
//! 1. the uniform offsets — the front end finds them by name from the linked program,
//!    so a rename in the WGSL that nothing followed shows up as one slider doing
//!    nothing and the rest being right;
//! 2. the orientation — GL's framebuffer origin is bottom-left and the shaders are
//!    authored for wgpu's top-left, and naga's `ADJUST_COORDINATE_SPACE` is what
//!    reconciles them. Both plans below must read *direct*, whatever their pass count:
//!    that is the invariant `preview.ts` relies on to flip once, at the blit, instead
//!    of tracking a parity that was wrong half the time;
//! 3. the maths, which is the least likely, because both sides ran the same file.

use std::path::PathBuf;

use half::f16;
use photodesk::engine::image::Image;
use photodesk::engine::render::{ADJUST_WGSL, ENCODE_WGSL, Renderer, encode_uniform_bytes};
use photodesk::engine::colour::SRGB;
use photodesk::photodesk::document::{
    AdjustV1, ColorSpace, Document, Layer, Op, Params, Source,
};
use photodesk::photodesk::graph;
use photodesk_renderer_spike::to_glsl_es300;
use naga::ShaderStage;

const N: u32 = 64;

fn generated() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/web/generated"))
}

/// A source with tone and chroma in it, so every stage has something to act on.
///
/// In-gamut for sRGB on purpose — the same precondition §12.2 states and for the same
/// reason: stage 13 gamut-maps (§16 #11), and a saturated fixture would make this a
/// measurement of the policy's kink rather than of the two renderers agreeing.
fn source() -> Image {
    let mut pixels = Vec::with_capacity((N * N * 3) as usize);
    for y in 0..N {
        for x in 0..N {
            let u = x as f32 / (N - 1) as f32;
            let v = y as f32 / (N - 1) as f32;
            pixels.push(f16::from_f32(0.08 + 0.55 * u));
            pixels.push(f16::from_f32(0.10 + 0.50 * v));
            pixels.push(f16::from_f32(0.12 + 0.40 * (1.0 - u * v)));
        }
    }
    Image::new(N, N, pixels)
}

/// Every one of v0.1's six sliders off its default, so a parameter bound to the wrong
/// offset cannot hide behind a zero.
fn document() -> Document {
    let mut doc = Document::new(Source {
        file: "harness.png".into(),
        hash: "blake3:0000000000000000000000000000000000000000000000000000000000000000".into(),
        dimensions: [N, N],
        colorspace: ColorSpace::DisplayP3,
        orientation: 1,
    });
    doc.output.colorspace = ColorSpace::Srgb;
    doc.stack.push(Layer {
        id: "global".into(),
        op: Op::Adjust,
        op_version: 1,
        enabled: true,
        name: None,
        opacity: None,
        mask: None,
        params: Params::AdjustV1(AdjustV1 {
            temperature: Some(-900.0),
            tint: None,
            exposure: Some(0.42),
            highlights: Some(-38.0),
            shadows: Some(27.0),
            blacks: Some(-15.0),
            contrast: Some(22.0),
            vibrance: None,
            saturation: None,
        }),
    });
    doc
}

#[test]
fn emit_the_front_ends_inputs_and_the_wgpu_reference() {
    let dir = generated();
    std::fs::create_dir_all(&dir).expect("create web/generated");

    // The shaders, lowered exactly as the app lowers them at startup.
    for (wgsl, name) in [(ADJUST_WGSL, "adjust"), (ENCODE_WGSL, "encode")] {
        for (entry, stage, ext) in [
            ("vs_main", ShaderStage::Vertex, "vert"),
            ("fs_main", ShaderStage::Fragment, "frag"),
        ] {
            let t = to_glsl_es300(wgsl, entry, stage)
                .unwrap_or_else(|e| panic!("{name}/{entry} failed to lower:\n{e}"));
            std::fs::write(dir.join(format!("plan-{name}.{ext}")), &t.source).expect("write");
        }
    }

    let image = source();
    // RGBA f16, byte for byte what `proxy_pixels` sends the webview.
    let mut rgba = Vec::with_capacity((N * N * 4 * 2) as usize);
    for rgb in image.pixels().chunks_exact(3) {
        for c in rgb {
            rgba.extend_from_slice(&c.to_le_bytes());
        }
        rgba.extend_from_slice(&f16::ONE.to_le_bytes());
    }
    std::fs::write(dir.join("plan-source.f16"), &rgba).expect("write source");

    let doc = document();

    // Two plans, because the preview blits two things and they do not have the same
    // number of passes. The edited document draws `adjust` then `encode`; §11's
    // hold-for-original draws the *same document with an empty stack*, which is one
    // pass. An orientation rule derived from the pass count is right for one of these
    // and wrong for the other, which is what happened: v0.1 presented every unedited
    // photograph upside down and this harness could not see it, because it only ever
    // ran the even case.
    let mut plain = doc.clone();
    plain.stack.clear();

    let mut plans = Vec::new();
    for (name, doc) in [("plan", &doc), ("plan-plain", &plain)] {
        let plan = graph::compile(doc).expect("compile");
        std::fs::write(
            dir.join(format!("{name}-graph.json")),
            serde_json::to_string_pretty(&plan).expect("serialise the plan"),
        )
        .expect("write plan");
        plans.push((name, plan));
    }
    std::fs::write(dir.join("plan-encode.bin"), encode_uniform_bytes(&SRGB)).expect("write uniform");

    // The reference: the same plans, through wgpu, at the same size.
    let renderer = match Renderer::new() {
        Ok(r) => r,
        Err(e) => {
            println!("no GPU here, so no reference was written: {e}");
            return;
        }
    };
    for (name, plan) in &plans {
        let rendered = renderer.render(plan, &image).expect("render");
        // Display-encoded 8-bit, which is what the preview's final target holds and what
        // a `readPixels` on the webview side will return.
        std::fs::write(dir.join(format!("{name}-reference.rgb8")), rendered.to_u8())
            .expect("write reference");
    }
    println!(
        "wrote {} — {}×{}, plans: {}",
        dir.display(),
        N,
        N,
        plans
            .iter()
            .map(|(name, plan)| format!("{name} ({} nodes)", plan.len()))
            .collect::<Vec<_>>()
            .join(", "),
    );
}
