//! Rendering a compiled graph, and §12.2's agreement test.
//!
//! There is no CPU reference here and there will not be one: two implementations of
//! one pipeline drift, which is what §0 freezes against. So these tests establish the
//! renderer the way the spec says to — by rendering things whose answer is known for a
//! reason other than "the shader said so".
//!
//! A GPU is an environment fact. Every test below skips with an explanation when there
//! is no adapter, rather than failing: a machine without one has told us nothing about
//! the renderer.

use half::f16;
use photodesk::engine::colour::{LINEAR_P3, SRGB};
use photodesk::engine::gamut::luma_weights;
use photodesk::engine::image::Image;
use photodesk::engine::render::{ADJUST_WGSL, RenderError, Rendered, Renderer};
use photodesk::photodesk::document::{
    AdjustV1, ColorSpace, Document, Layer, Mask, MaskComponent, MaskOp, Op, Params, Source,
};
use photodesk::photodesk::graph;

/// Skips rather than fails when there is no GPU, and says so once.
macro_rules! renderer {
    () => {
        match Renderer::new() {
            Ok(r) => {
                eprintln!("adapter: {}", r.adapter);
                r
            }
            Err(e) => {
                println!("SKIP: {e}");
                return;
            }
        }
    };
}

fn source() -> Source {
    Source {
        file: "IMG_4821.HEIC".into(),
        hash: "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        dimensions: [64, 64],
        colorspace: ColorSpace::DisplayP3,
        orientation: 1,
    }
}

fn layer(id: &str, params: AdjustV1) -> Layer {
    Layer {
        id: id.into(),
        op: Op::Adjust,
        op_version: 1,
        enabled: true,
        name: None,
        opacity: None,
        mask: None,
        params: Params::AdjustV1(params),
    }
}

fn doc(layers: Vec<Layer>) -> Document {
    Document { stack: layers, ..Document::new(source()) }
}

/// A test image with real structure: a hue sweep, a luminance ramp, and fine detail
/// that only exists above the proxy's resolution.
///
/// The last part is what §12.2 is about. Smooth content agrees trivially at any
/// resolution; the interesting case is detail the proxy cannot hold.
fn test_image(size: u32) -> Image {
    let mut pixels = vec![f16::ZERO; (size * size * 3) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / (size - 1) as f32;
            let v = y as f32 / (size - 1) as f32;
            let angle = u * std::f32::consts::TAU;
            // A one-pixel checker on top, so there is detail at the Nyquist limit.
            let detail = if (x + y) % 2 == 0 { 0.06 } else { -0.06 };
            let base = 0.02 + v * 0.85 + detail;
            let i = ((y * size + x) * 3) as usize;
            pixels[i] = f16::from_f32((base * (1.0 + 0.5 * angle.cos())).clamp(0.0, 1.2));
            pixels[i + 1] =
                f16::from_f32((base * (1.0 + 0.5 * (angle - 2.094).cos())).clamp(0.0, 1.2));
            pixels[i + 2] =
                f16::from_f32((base * (1.0 + 0.5 * (angle + 2.094).cos())).clamp(0.0, 1.2));
        }
    }
    Image::new(size, size, pixels)
}

/// The largest channel difference between two renders of the same size.
fn worst(a: &Rendered, b: &Rendered) -> f32 {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut worst = 0.0f32;
    for y in 0..a.height() {
        for x in 0..a.width() {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            for c in 0..3 {
                worst = worst.max((p[c] - q[c]).abs());
            }
        }
    }
    worst
}

// ------------------------------------------------- the shader's own constants

/// `adjust.wgsl` hardcodes the working space's luminance weights and its XYZ matrix,
/// because a uniform would have to be filled by both the exporter and the preview —
/// two places to write one number. This is the test that keeps them tied to the
/// derived values in `colour.rs`, so the hardcoding is a copy that cannot drift rather
/// than a copy nobody is watching.
///
/// Needs no GPU: it reads the shader as text.
#[test]
fn the_shaders_working_space_constants_match_the_derived_ones() {
    let numbers = |after: &str| -> Vec<f64> {
        let at = ADJUST_WGSL.find(after).unwrap_or_else(|| panic!("no `{after}` in the shader"));
        let tail = &ADJUST_WGSL[at..];
        let end = tail.find(';').expect("a terminated declaration");
        tail[..end]
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|t| t.contains('.'))
            .filter_map(|t| t.parse::<f64>().ok())
            .collect()
    };

    let luma = numbers("const LUMA");
    let want = luma_weights(&LINEAR_P3);
    println!("shader LUMA {luma:?}\nderived     {want:?}");
    assert_eq!(luma.len(), 3, "expected three luminance weights");
    for (got, want) in luma.iter().zip(want) {
        assert!(
            (got - want).abs() < 1e-6,
            "the shader's luminance weights are not linear Display P3's: {got} vs {want}"
        );
    }

    // XYZ → linear P3, column-major in WGSL, so the flattened list is the transpose of
    // our row-major matrix.
    let m = numbers("const XYZ_TO_P3");
    let want = LINEAR_P3.from_xyz().0;
    assert_eq!(m.len(), 9, "expected nine matrix entries");
    for col in 0..3 {
        for row in 0..3 {
            let got = m[col * 3 + row];
            assert!(
                (got - want[row][col]).abs() < 1e-5,
                "the shader's XYZ→P3 matrix disagrees at ({row},{col}): {got} vs {}",
                want[row][col]
            );
        }
    }
    println!("both constants match the values derived from chromaticities");
}

// ---------------------------------------------------------------- the identity

/// A document with no edits renders the photograph back.
///
/// The chain is real — upload, encode pass, readback — so this is the test that the
/// plumbing is right before anything is asked about the maths.
#[test]
fn an_unedited_document_renders_the_photograph_back() {
    let r = renderer!();
    let image = test_image(32);
    let g = graph::compile(&doc(vec![])).expect("compile");
    let out = r.render(&g, &image).expect("render");
    println!("{out:?}");

    // The document exports sRGB by default, so the comparison has to go through the
    // same matrix and curve — computed here from `colour.rs` rather than from the
    // shader, which is the only sense in which this has a reference.
    let matrix = LINEAR_P3.linear_to(&SRGB);
    let policy = photodesk::engine::gamut::EXPORT_GAMUT_POLICY;
    let mut biggest = 0.0f32;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mapped = policy.map(matrix.apply(image.pixel(x, y)), &SRGB);
            let want = [
                SRGB.transfer.from_linear(mapped[0]),
                SRGB.transfer.from_linear(mapped[1]),
                SRGB.transfer.from_linear(mapped[2]),
            ];
            let got = out.pixel(x, y);
            for c in 0..3 {
                biggest = biggest.max((got[c] - want[c]).abs());
            }
        }
    }
    println!("worst channel difference from the CPU-computed encode: {biggest:.3e}");
    // One step of the RGBA16F attachment's own grid at the top of the range, which is
    // what `encode_stage.rs` already measured and attributed to the driver truncating
    // rather than rounding.
    assert!(biggest < 6.0e-4, "the identity render is off by {biggest:.3e}");
}

/// **A slider at its default does exactly nothing.**
///
/// Not "nearly nothing". Every control here is written so that its zero is the
/// identity by construction, and temperature is the one where that took work: the
/// white-balance gain is the ratio of the target white to *the daylight locus
/// evaluated at the reference*, not to D65's defined chromaticity, so the ratio at
/// zero is 1.0 per channel rather than within a thousandth of it.
#[test]
fn every_control_at_zero_is_the_identity() {
    let r = renderer!();
    let image = test_image(32);

    let plain = r
        .render(&graph::compile(&doc(vec![])).unwrap(), &image)
        .expect("render");

    // Present in the document — so the compiler does not eliminate the layer — and
    // set to the value the UI shows when the slider has not been touched.
    let zeroed = AdjustV1 {
        temperature: Some(0.0),
        tint: Some(0.0),
        exposure: Some(0.0),
        highlights: Some(0.0),
        shadows: Some(0.0),
        blacks: Some(0.0),
        contrast: Some(0.0),
        vibrance: Some(0.0),
        saturation: Some(0.0),
    };
    let g = graph::compile(&doc(vec![layer("global", zeroed)])).unwrap();
    assert!(
        g.nodes().iter().any(|n| n.kind.tag() == "adjust"),
        "the layer was eliminated, so this test is comparing a graph to itself"
    );
    let adjusted = r.render(&g, &image).expect("render");

    let d = worst(&plain, &adjusted);
    println!("worst channel difference with every control at zero: {d:.3e}");
    assert!(
        d < 1.0e-5,
        "a stack of untouched sliders changed the picture by {d:.3e}. The one to \
         suspect is temperature: its zero is only the identity if the gain is measured \
         against the locus rather than against D65's defined chromaticity"
    );
}

/// Exposure is in stops, which is the one formulation in the set that is not a choice.
#[test]
fn exposure_is_stops() {
    let r = renderer!();
    // A flat mid-grey, so the assertion is about the arithmetic rather than about
    // where a gradient happens to sit.
    let image = Image::new(8, 8, vec![f16::from_f32(0.18); 8 * 8 * 3]);

    for stops in [-1.0f32, -0.5, 0.5, 1.0] {
        let g = graph::compile(&doc(vec![layer(
            "global",
            AdjustV1 { exposure: Some(stops), ..Default::default() },
        )]))
        .unwrap();
        let out = r.render(&g, &image).expect("render");

        // Back through the encode, to the linear working value the shader produced.
        let encoded = out.pixel(4, 4)[1];
        let linear_srgb = SRGB.transfer.to_linear(encoded);
        // A neutral is a neutral in both spaces, so the matrix leaves it alone and the
        // green channel is the working value directly.
        let want = 0.18 * stops.exp2();
        println!("{stops:+.1} stops: 0.18 -> {linear_srgb:.5}, expected {want:.5}");
        assert!(
            (linear_srgb - want).abs() < 2e-3,
            "{stops:+} stops moved 0.18 to {linear_srgb}, not {want}"
        );
    }
}

/// Warming a photograph must not also brighten it: §11 gives each control one job, and
/// brightness is exposure's.
#[test]
fn white_balance_preserves_luminance() {
    let r = renderer!();
    let image = Image::new(8, 8, vec![f16::from_f32(0.18); 8 * 8 * 3]);
    let weights = luma_weights(&SRGB);

    for temperature in [-2000.0f32, -500.0, 500.0, 2000.0] {
        let g = graph::compile(&doc(vec![layer(
            "global",
            AdjustV1 { temperature: Some(temperature), ..Default::default() },
        )]))
        .unwrap();
        let out = r.render(&g, &image).expect("render");
        let p = out.pixel(4, 4);
        let linear: Vec<f64> = p.iter().map(|v| SRGB.transfer.to_linear(*v) as f64).collect();
        let y: f64 = (0..3).map(|c| linear[c] * weights[c]).sum();

        println!("{temperature:+.0} K: {p:.4?}  luminance {y:.5}");
        assert!(
            (y - 0.18).abs() < 5e-3,
            "{temperature:+} K changed luminance from 0.18 to {y:.5}"
        );
        // …and it did do something, or the assertion above is about nothing.
        assert!(
            (p[0] - p[2]).abs() > 0.005,
            "{temperature:+} K left red and blue equal; the control did nothing"
        );
    }
    // Warmer is redder. The sign of the control is the half nobody notices is wrong
    // until they use it.
    let render_at = |t: f32| {
        let g = graph::compile(&doc(vec![layer(
            "global",
            AdjustV1 { temperature: Some(t), ..Default::default() },
        )]))
        .unwrap();
        r.render(&g, &image).expect("render").pixel(4, 4)
    };
    let (cool, warm) = (render_at(2000.0), render_at(-2000.0));
    println!("+2000 K {cool:.4?}   -2000 K {warm:.4?}");
    assert!(
        warm[0] > cool[0] && warm[2] < cool[2],
        "a negative temperature delta is supposed to warm the picture: {warm:?} vs {cool:?}"
    );
}

// ----------------------------------------------------- what is not built yet

/// A node kind the renderer cannot run is refused **by name**, not skipped.
///
/// Rendering a masked document without its mask produces a picture that looks
/// plausible and is wrong, which is the same reason §6.3 rejects an unknown `op`
/// rather than ignoring it.
#[test]
fn unimplemented_node_kinds_are_refused_rather_than_skipped() {
    let r = renderer!();
    let image = test_image(16);

    let masked = doc(vec![Layer {
        mask: Some(Mask {
            op: MaskOp::Union,
            components: vec![MaskComponent::Linear {
                from: [0.5, 0.0],
                to: [0.5, 0.42],
                feather: None,
                invert: None,
            }],
        }),
        ..layer("sky", AdjustV1 { exposure: Some(-0.4), ..Default::default() })
    }]);
    let err = r
        .render(&graph::compile(&masked).unwrap(), &image)
        .expect_err("masks are v0.4");
    println!("{err}");
    assert!(matches!(err, RenderError::Unimplemented { .. }));
    assert!(err.to_string().contains("v0.4"));

    // Opacity alone is enough to need a composite, and that is v0.4 too.
    let faded = doc(vec![Layer {
        opacity: Some(0.5),
        ..layer("a", AdjustV1 { exposure: Some(0.3), ..Default::default() })
    }]);
    assert!(matches!(
        r.render(&graph::compile(&faded).unwrap(), &image),
        Err(RenderError::Unimplemented { .. })
    ));
}

// -------------------------------------------------------------------- §12.2

/// §12.2, the test the specification says the one-shader-source invariant is a comment
/// without:
///
/// > Render the same document at proxy and at full-res-downsampled-to-proxy. Assert
/// > they match within threshold.
///
/// ## What the threshold has to be about
///
/// The naive expectation is near-exact agreement, since `adjust` v1 has none of the
/// spatial stages §12.2's carve-outs are written for. That expectation is wrong, and
/// the first version of this test failed at **57 codes with no adjustment at all** —
/// which is worth recording because the wrong diagnosis was reached twice before the
/// right one.
///
/// It is not the adjustment chain. It is not clipping either (that hypothesis
/// predicted the disagreement would sit at the rails; it sits away from them). It is
/// **the gamut map**. Proxy editing computes `f(mean(pixels))` where export computes
/// `mean(f(pixels))`, and §16 #11's constant-luminance chroma clip is emphatically not
/// linear: two neighbouring pixels, one well outside sRGB and one inside, are worth
/// about 18 codes of disagreement on their own. Any gamut policy has this — the
/// per-channel clip has it too — and so does every editor that has proxied since 2007.
///
/// So the disagreement lives **entirely in detail finer than the proxy**, and that is
/// what the two fixtures below separate. It is also why it is not a lie the user can
/// see: §7.1 sizes the proxy at twice the viewport, so detail the proxy cannot hold is
/// detail the screen cannot show, and zooming in makes the proxy finer.
///
/// The regression test is therefore on **band-limited** content — smooth enough that
/// the reduction loses nothing — where the two paths see the same pixels and any
/// disagreement really is resolution-dependence in the chain.
#[test]
fn proxy_and_full_res_agree() {
    let r = renderer!();
    const FULL: u32 = 256;
    const PROXY: u32 = 64;

    let document = doc(vec![layer(
        "global",
        AdjustV1 {
            exposure: Some(0.35),
            contrast: Some(22.0),
            highlights: Some(-35.0),
            shadows: Some(42.0),
            blacks: Some(-12.0),
            temperature: Some(600.0),
            ..Default::default()
        },
    )]);
    let edited = graph::compile(&document).expect("compile");
    let identity = graph::compile(&doc(vec![])).expect("compile");

    let smooth = disagreement(&r, &edited, &band_limited(FULL), PROXY);
    // The preconditions are asserted rather than assumed. If the fixture ever drifts
    // out of gamut or grows fine detail, the bar below stops being about the renderer
    // and starts being about an inherent property, silently.
    {
        let out = r.render(&edited, &band_limited(FULL)).expect("render");
        let mut extreme = 0.0f32;
        for y in 0..FULL {
            for x in 0..FULL {
                for c in out.pixel(x, y) {
                    extreme = extreme.max((c - 0.5).abs());
                }
            }
        }
        assert!(
            extreme < 0.48,
            "the band-limited fixture reaches {:.3} from mid-grey, which is close \
             enough to a rail that the gamut map may be active — see the fixture's own \
             two preconditions",
            extreme
        );
    }
    let detailed = disagreement(&r, &edited, &fine_detail(FULL), PROXY);
    // The attribution: the same detailed content with *no adjustment*, so the only
    // non-linear thing left between the two paths is stage 13's gamut map.
    let stage_13_only = disagreement(&r, &identity, &fine_detail(FULL), PROXY);

    println!(
        "§12.2  {FULL}² rendered then reduced, against {PROXY}² reduced then rendered\n\
         \x20                                       worst      mean\n\
         \x20      band-limited, full chain    {:>8.4}  {:>8.4}   <- the regression bar\n\
         \x20      fine detail,  full chain    {:>8.2}  {:>8.3}\n\
         \x20      fine detail,  stage 13 only {:>8.2}  {:>8.3}   <- the gamut map alone\n\
         \x20      all figures in 8-bit codes",
        smooth.worst * 255.0,
        smooth.mean * 255.0,
        detailed.worst * 255.0,
        detailed.mean * 255.0,
        stage_13_only.worst * 255.0,
        stage_13_only.mean * 255.0,
    );

    // **The bar.** On content the proxy can hold, the two paths see the same pixels,
    // so they must produce the same answer. At `op_version` 1 the chain has no way to
    // depend on resolution at all, and a spatial stage that did would show up here
    // first — which is exactly what §12.2 exists to catch.
    assert!(
        smooth.worst * 255.0 < 1.0,
        "on band-limited content proxy and export differ by {:.4} of an 8-bit code. \
         The reduction loses nothing there, so both paths see the same pixels and the \
         chain has produced a resolution-dependent answer",
        smooth.worst * 255.0
    );

    // **And the finding, asserted so it cannot quietly stop being true.** Detail finer
    // than the proxy costs two orders of magnitude more, and most of it is stage 13.
    assert!(
        detailed.worst > smooth.worst * 20.0,
        "fine detail was expected to dominate and did not: {:.3} codes against {:.4}. \
         If that is now true the fixtures have converged and this measurement means \
         nothing",
        detailed.worst * 255.0,
        smooth.worst * 255.0
    );
    assert!(
        stage_13_only.worst > detailed.worst * 0.5,
        "the gamut map was expected to account for most of the fine-detail \
         disagreement ({:.2} codes) and accounted for {:.2}. The attribution in this \
         test's documentation is then wrong and should be re-read before it is trusted",
        detailed.worst * 255.0,
        stage_13_only.worst * 255.0
    );
}

/// Render `graph` at full size and at proxy size, reduce the first, and report how far
/// apart they are.
///
/// The reduction happens in **linear light**: the render is encoded, so the transfer
/// curve is inverted, the average taken, and the curve reapplied. Averaging encoded
/// values would introduce a darkening of its own and charge it to the renderer.
fn disagreement(
    r: &Renderer,
    g: &photodesk::photodesk::graph::Graph,
    full_image: &Image,
    proxy: u32,
) -> Disagreement {
    let full = full_image.width();
    let exported = r.render(g, full_image).expect("full-res render");
    let previewed = r.render(g, &full_image.proxy(proxy)).expect("proxy render");

    let space = exported.space();
    let mut linear = vec![f16::ZERO; (full * full * 3) as usize];
    for y in 0..full {
        for x in 0..full {
            let p = exported.pixel(x, y);
            let i = ((y * full + x) * 3) as usize;
            for c in 0..3 {
                linear[i + c] = f16::from_f32(space.transfer.to_linear(p[c]));
            }
        }
    }
    let reduced = Image::new(full, full, linear).proxy(proxy);

    let (mut worst, mut total) = (0.0f32, 0.0f64);
    for y in 0..proxy {
        for x in 0..proxy {
            let a = reduced.pixel(x, y);
            let b = previewed.pixel(x, y);
            for c in 0..3 {
                let d = (space.transfer.from_linear(a[c]) - b[c]).abs();
                worst = worst.max(d);
                total += d as f64;
            }
        }
    }
    Disagreement {
        worst,
        mean: (total / (proxy * proxy * 3) as f64) as f32,
    }
}

struct Disagreement {
    worst: f32,
    mean: f32,
}

/// Content meeting **both** preconditions for exact agreement.
///
/// There are two ways the two paths can differ, and a fixture that fails either one
/// stops being a regression test and becomes a measurement of an inherent property:
///
/// 1. **Detail finer than the proxy**, which makes `mean` lossy, so `f∘mean` and
///    `mean∘f` differ for any non-linear `f`. Avoided by a low-frequency wave whose
///    finest feature is far wider than the 4:1 reduction.
/// 2. **Content outside the destination gamut**, where §16 #11's map has a derivative
///    discontinuity at the boundary — averaging across that kink diverges even on
///    perfectly smooth content. Avoided by staying near the neutral axis, so nothing
///    leaves sRGB before or after the adjustment.
///
/// With both held, `mean` is the identity and the chain is smooth, so the two paths
/// have no way to disagree at all. Whatever is left is the renderer, which is what
/// §12.2 is for.
fn band_limited(size: u32) -> Image {
    let mut pixels = vec![f16::ZERO; (size * size * 3) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / (size - 1) as f32;
            let v = y as f32 / (size - 1) as f32;
            let i = ((y * size + x) * 3) as usize;
            // A mid-range ramp with a gentle, low-amplitude colour cast: enough that
            // the channels are not identical, little enough that P3 → sRGB never sends
            // one out of range.
            let base = 0.10 + 0.30 * v;
            for c in 0..3 {
                let phase = 2.094 * c as f32;
                let cast = 0.03 * (u * std::f32::consts::PI + phase).sin();
                pixels[i + c] = f16::from_f32((base + cast).clamp(0.0, 1.0));
            }
        }
    }
    Image::new(size, size, pixels)
}

/// The same gradients with a full-swing one-pixel checkerboard on top: detail the
/// proxy cannot hold, and saturated enough that most of it leaves sRGB.
fn fine_detail(size: u32) -> Image {
    let mut pixels = vec![f16::ZERO; (size * size * 3) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / (size - 1) as f32;
            let v = y as f32 / (size - 1) as f32;
            let angle = u * std::f32::consts::TAU;
            let on = (x + y) % 2 == 0;
            let base = 0.05 + v * 0.7;
            let i = ((y * size + x) * 3) as usize;
            for c in 0..3 {
                let hue = 1.0 + 0.7 * (angle - 2.094 * c as f32).cos();
                let swing = if on { 1.0 } else { 0.15 };
                pixels[i + c] = f16::from_f32((base * hue * swing).clamp(0.0, 1.4));
            }
        }
    }
    Image::new(size, size, pixels)
}

/// The renderer asks for the limits WebGL2 guarantees, so a graph that renders in the
/// exporter renders in the preview.
///
/// Asking for more would let the export path succeed on something the webview refuses,
/// which is the WYSIWYG drift §0 freezes against arriving through the back door.
#[test]
fn the_renderer_stays_inside_what_webgl2_guarantees() {
    let r = renderer!();
    // A six-layer stack, which is §7.3's stated bound.
    let layers: Vec<Layer> = (0..6)
        .map(|i| {
            layer(
                &format!("l{i}"),
                AdjustV1 {
                    exposure: Some(0.05 * i as f32),
                    saturation: Some(2.0 * i as f32),
                    ..Default::default()
                },
            )
        })
        .collect();
    let g = graph::compile(&doc(layers)).expect("compile");
    println!("{}", g.describe());

    let image = test_image(64);
    let out = r.render(&g, &image).expect("six layers render");
    assert_eq!((out.width(), out.height()), (64, 64));

    // Every output value is a real number in range. A pipeline that produced a NaN
    // would still "render", and the NaN would reach the exported file.
    for y in 0..out.height() {
        for x in 0..out.width() {
            for c in out.pixel(x, y) {
                assert!(c.is_finite(), "a non-finite value reached the output at ({x},{y})");
                assert!((-0.001..=1.001).contains(&c), "an out-of-range value {c} reached the output");
            }
        }
    }
}
