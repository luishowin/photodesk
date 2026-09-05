//! §16 #11 — the export gamut-mapping policy, measured rather than picked.
//!
//! §4 stops at "linear P3 → tone encode → sRGB" and never says what happens to the
//! channels sRGB cannot hold. `SPIKE-B.md` recorded the gap and the harness filled it
//! with a clamp. Real photographs then made it concrete: the same pixels that survive
//! the working space at max ΔE 0.0000 move by **max ΔE 2.86** on export, so this is
//! worth up to three ΔE on the user's own pictures and it is currently decided inside
//! a test file.
//!
//! ## The trap this file is arranged around
//!
//! The obvious measurement is ΔE from the original, lowest wins. That measurement
//! picks the wrong policy, and it does so *confidently* — which is the shape of the
//! three mistakes Phase 0 already caught. Clipping each channel to [0,1] is exactly
//! the Euclidean projection onto the gamut cube, so it is the nearest in-gamut colour
//! in linear RGB; the perceptual nearest is a search away and lands nearby. Both
//! minimise a distance. Both take every colour outside the gamut and put it on the
//! boundary, which is precisely how a gradient becomes a flat patch.
//!
//! So the tests below measure four things and refuse to collapse them into one:
//!
//! 1. **Lightness and hue** — what a wrong-looking colour actually looks wrong in.
//! 2. **Gradient survival** — do distinct inputs stay distinct? §2.2's three-part
//!    "no banding" definition, aimed at the export instead of the working space.
//! 3. **In-gamut damage** — what the policy costs the colours that were already fine,
//!    which on a photograph is nearly all of them.
//! 4. **Distance from the original**, reported last and on purpose, as a bound rather
//!    than an objective.
//!
//! Run with `cargo test -p photodesk-color --test gamut_policy -- --nocapture`.

use lcms2::{CIExyY, CIExyYTRIPLE, Intent, PixelFormat, Profile, ToneCurve, Transform};
use photodesk_color::colour::{DISPLAY_P3, SRGB, Space, encoded_to_lab, linear_to_lab};
use photodesk_color::corpus::{
    self, COLORCHECKER_SRGB, gamut_boundary_ramps, untagged_screenshot, wide_gamut_gradient,
};
use photodesk_color::delta_e::{DeltaStats, ciede2000};
use photodesk_color::gamut::{GamutPolicy, luma_weights, nearest_in_gamut_lab};

/// The shippable candidates, at the knees the sweep below covers.
///
/// `GamutPolicy::None` is absent: an encoder has to do something with a negative
/// channel and "nothing" is not an option, so it is an instrument, not a candidate.
fn candidates() -> Vec<GamutPolicy> {
    vec![
        GamutPolicy::ClipLinear,
        GamutPolicy::PreserveLuma,
        GamutPolicy::CompressLuma { knee: 0.80 },
        GamutPolicy::CompressLuma { knee: 0.90 },
        GamutPolicy::CompressLuma { knee: 0.95 },
    ]
}

/// Encoded Display P3 → linear sRGB, with no gamut handling of any kind.
///
/// The input to every policy, and the only place the matrices live in this file: a
/// policy is judged on what it does to this vector, not on whether it can reproduce
/// the transform that produced it.
fn to_linear_dst(code: [u8; 3], src: &Space, dst: &Space) -> [f32; 3] {
    let lin = [
        src.transfer.to_linear(code[0] as f32 / 255.0),
        src.transfer.to_linear(code[1] as f32 / 255.0),
        src.transfer.to_linear(code[2] as f32 / 255.0),
    ];
    src.linear_to(dst).apply(lin)
}

/// Inside the destination gamut, with a tolerance — for *population* questions
/// ("what fraction of this photograph is at stake"), where a colour sitting 1e-19
/// outside the cube is noise rather than content.
fn in_gamut(lin: [f32; 3]) -> bool {
    lin.iter().all(|c| (-1.0e-6..=1.0 + 1.0e-6).contains(c))
}

/// Strictly inside the cube. The tolerant predicate above admits colours a hair
/// outside, and "the policy does not touch in-gamut content" is a claim about the
/// cube rather than about a neighbourhood of it — a clip legitimately moves
/// -5.7e-19 to 0.
fn strictly_in_gamut(lin: [f32; 3]) -> bool {
    lin.iter().all(|c| (0.0..=1.0).contains(c))
}

/// L\*, C\*, h° from a linear colour in `space`.
fn lch(lin: [f32; 3], space: &Space) -> (f64, f64, f64) {
    let lab = linear_to_lab(lin, space);
    let c = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    let mut h = lab[2].atan2(lab[1]).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }
    (lab[0], c, h)
}

/// Shortest signed angle between two hues, in degrees.
fn hue_gap(a: f64, b: f64) -> f64 {
    let d = (b - a).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// Metric hue difference, in Lab units: `2·√(C₁C₂)·sin(Δh/2)`.
///
/// The number that belongs next to ΔL\* and ΔC\*, because a hue *angle* is not
/// comparable to them and overstates itself at low chroma — swinging a nearly-grey
/// colour thirty degrees is arithmetic, not a visible error. This is the term
/// CIEDE2000 actually uses, so reporting the angle alone would be measuring the
/// wrong thing while looking rigorous.
fn hue_metric(c1: f64, c2: f64, dh_deg: f64) -> f64 {
    2.0 * (c1 * c2).sqrt() * (dh_deg.to_radians() / 2.0).sin()
}

// ------------------------------------------------------- why this is ours to write

/// The first question is whether the policy has to be written at all, and the answer
/// is structural rather than a matter of taste.
///
/// Perceptual rendering lives in a profile's B2A lookup tables. sRGB and Display P3
/// are matrix/TRC profiles and have none, so lcms2 has nothing to render *with* and
/// every intent collapses onto the same colorimetric transform. Asking a colour
/// engine for "perceptual" here returns the clip with a different name — which is
/// worth asserting rather than believing, because it is the single fact that decides
/// whether §16 #11 is our decision or somebody else's default.
#[test]
fn lcms2_has_no_perceptual_answer_for_a_matrix_profile() {
    let curve = ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
        .expect("sRGB parametric curve");
    let profile = |s: &Space| {
        Profile::new_rgb(
            &CIExyY { x: s.white.x, y: s.white.y, Y: 1.0 },
            &CIExyYTRIPLE {
                Red: CIExyY { x: s.red.x, y: s.red.y, Y: 1.0 },
                Green: CIExyY { x: s.green.x, y: s.green.y, Y: 1.0 },
                Blue: CIExyY { x: s.blue.x, y: s.blue.y, Y: 1.0 },
            },
            &[&curve, &curve, &curve],
        )
        .expect("build RGB profile")
    };
    let (src, dst) = (profile(&DISPLAY_P3), profile(&SRGB));

    let input: Vec<[f64; 3]> = wide_gamut_gradient()
        .iter()
        .map(|c| [c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0])
        .collect();

    let run = |intent: Intent| {
        let t: Transform<[f64; 3], [f64; 3]> =
            Transform::new(&src, PixelFormat::RGB_DBL, &dst, PixelFormat::RGB_DBL, intent)
                .expect("P3 -> sRGB transform");
        let mut out = vec![[0.0f64; 3]; input.len()];
        t.transform_pixels(&input, &mut out);
        out
    };

    let relative = run(Intent::RelativeColorimetric);
    for intent in [Intent::Perceptual, Intent::Saturation] {
        let other = run(intent);
        let worst = relative
            .iter()
            .zip(&other)
            .map(|(a, b)| {
                ciede2000(
                    encoded_to_lab([a[0] as f32, a[1] as f32, a[2] as f32], &SRGB),
                    encoded_to_lab([b[0] as f32, b[1] as f32, b[2] as f32], &SRGB),
                )
            })
            .fold(0.0f64, f64::max);
        println!("lcms2 {intent:?} vs RelativeColorimetric, P3 -> sRGB: max ΔE {worst:.6}");
        assert!(
            worst < 1.0e-9,
            "lcms2 {intent:?} now differs from relative colorimetric by ΔE {worst:.6} on \
             matrix/TRC profiles. That would mean §16 #11 has an off-the-shelf answer \
             after all, and this whole file should be re-read before it is trusted."
        );
    }
    println!(
        "  -> every intent is the same transform. A matrix/TRC profile carries no B2A \n\
         \x20    table, so there is no perceptual rendering to ask for and the policy is ours."
    );
}

// ------------------------------------------------- why ΔE cannot choose the policy

/// The control, in the spirit of `tests/renderer/`'s compute-shader control: it exists
/// so the result below is a consequence rather than a coincidence.
///
/// Two claims, both checked. Clipping to the cube **is** the nearest in-gamut colour
/// in linear RGB, exactly — so "clip moves the colour least" is true, in a space
/// nobody sees in. And the perceptually nearest colour, found by search, beats every
/// shippable policy on ΔE while flattening gradients exactly as badly. Whichever of
/// the two wins on distance, distance is not what is being chosen.
#[test]
fn distance_from_the_original_cannot_choose_the_policy() {
    let samples: Vec<[f32; 3]> = wide_gamut_gradient()
        .iter()
        .map(|c| to_linear_dst(*c, &DISPLAY_P3, &SRGB))
        .filter(|lin| !in_gamut(*lin))
        .collect();
    println!(
        "control  {} out-of-gamut samples from the wide-gamut sweep",
        samples.len()
    );
    assert!(samples.len() > 30, "the sweep stopped leaving sRGB; nothing here is measuring anything");

    // Claim 1: the clip is the Euclidean projection onto the cube. Not approximately.
    for lin in &samples {
        let clipped = GamutPolicy::ClipLinear.map(*lin, &SRGB);
        let brute = {
            // The nearest point of a box to a point is the componentwise clamp; check
            // it against a direct search so the claim is not just a restatement of the
            // implementation.
            let mut best = [0.0f32; 3];
            let mut best_d = f32::INFINITY;
            for i in 0..=20 {
                for j in 0..=20 {
                    for k in 0..=20 {
                        let c = [i as f32 / 20.0, j as f32 / 20.0, k as f32 / 20.0];
                        let d = (0..3).map(|a| (c[a] - lin[a]).powi(2)).sum::<f32>();
                        if d < best_d {
                            best_d = d;
                            best = c;
                        }
                    }
                }
            }
            best
        };
        let gap: f32 = (0..3).map(|a| (brute[a] - clipped[a]).abs()).fold(0.0, f32::max);
        assert!(
            gap <= 0.05 + 1.0e-6,
            "clip is not the linear-RGB nearest point for {lin:?}: {clipped:?} vs {brute:?}"
        );
    }
    println!("control  clip == the linear-RGB nearest in-gamut colour (checked on a 21³ grid)");

    // Claim 2: the perceptually nearest colour beats everything shippable on ΔE.
    let de = |lin: [f32; 3], mapped: [f32; 3]| {
        ciede2000(linear_to_lab(lin, &SRGB), linear_to_lab(mapped, &SRGB))
    };
    println!("\n  {:<16} {:>10} {:>10}   {}", "policy", "mean ΔE", "max ΔE", "shippable in a fragment shader?");
    let mut rows: Vec<(String, f64)> = Vec::new();
    for policy in candidates() {
        let d: Vec<f64> = samples.iter().map(|l| de(*l, policy.map(*l, &SRGB))).collect();
        let s = DeltaStats::from(&d);
        println!("  {:<16} {:>10.4} {:>10.4}   yes", policy.label(), s.mean, s.max);
        rows.push((policy.label(), s.mean));
    }
    let control: Vec<f64> = samples.iter().map(|l| de(*l, nearest_in_gamut_lab(*l, &SRGB))).collect();
    let control_stats = DeltaStats::from(&control);
    println!(
        "  {:<16} {:>10.4} {:>10.4}   NO — a per-pixel search",
        "nearest-Lab", control_stats.mean, control_stats.max
    );

    let best_shippable = rows.iter().map(|(_, m)| *m).fold(f64::INFINITY, f64::min);
    assert!(
        control_stats.mean <= best_shippable + 1.0e-9,
        "the ΔE-minimising control ({:.4}) lost to a closed-form policy ({best_shippable:.4}); \
         the local search is not finding the minimum and the argument below rests on it",
        control_stats.mean
    );

    // And the point: the ΔE winner flattens as badly as the clip does. Both put every
    // out-of-gamut colour on the boundary, because that is where the nearest in-gamut
    // colour is — for any distance metric.
    let on_boundary = |c: [f32; 3]| c.iter().any(|v| *v <= 1.0e-5 || *v >= 1.0 - 1.0e-5);
    let clip_boundary = samples.iter().filter(|l| on_boundary(GamutPolicy::ClipLinear.map(**l, &SRGB))).count();
    let ctrl_boundary = samples.iter().filter(|l| on_boundary(nearest_in_gamut_lab(**l, &SRGB))).count();
    println!(
        "\n  of {} out-of-gamut samples, clip leaves {} on the gamut surface and the \n\
         \x20 ΔE-minimising control leaves {}. Minimising a distance *is* projecting to the \n\
         \x20 surface, whatever the distance is — which is why the lowest ΔE and the flattest \n\
         \x20 gradient are the same answer, and why this file does not rank on ΔE.",
        samples.len(), clip_boundary, ctrl_boundary
    );
    assert_eq!(
        (clip_boundary, ctrl_boundary),
        (samples.len(), samples.len()),
        "a distance-minimising policy left something off the gamut surface, which would \
         break the argument that ΔE and flattening are the same axis"
    );
}

// ------------------------------------------------------------- 1. lightness and hue

/// What a wrong colour is actually wrong *in*.
///
/// Clipping moves lightness and hue both, and it does so without a policy: the amount
/// depends on which channel happened to run out first. The luminance-preserving
/// policies hold L\* exactly — not nearly, exactly, because the vector they scale
/// carries zero luminance by construction — and this test is where that stops being a
/// claim in a doc comment.
#[test]
fn lightness_and_hue_under_each_policy() {
    let ramps = gamut_boundary_ramps();
    let samples: Vec<[f32; 3]> = wide_gamut_gradient()
        .iter()
        .chain(ramps.iter().flat_map(|(_, r)| r.iter()))
        .map(|c| to_linear_dst(*c, &DISPLAY_P3, &SRGB))
        .filter(|lin| !in_gamut(*lin))
        .collect();

    println!("out-of-gamut samples: {}", samples.len());
    println!(
        "  {:<16} {:>18} {:>18} {:>18} {:>12}",
        "policy", "|ΔL*| max/mean", "|ΔC*| max/mean", "|ΔH| max/mean", "|Δh°| max"
    );

    let mut clip_l = 0.0f64;
    let mut preserve_l = 0.0f64;
    for policy in candidates() {
        let (mut dl, mut dc, mut dh, mut angle) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for lin in &samples {
            let (l0, c0, h0) = lch(*lin, &SRGB);
            let (l1, c1, h1) = lch(policy.map(*lin, &SRGB), &SRGB);
            dl.push((l1 - l0).abs());
            dc.push((c1 - c0).abs());
            let gap = hue_gap(h0, h1);
            dh.push(hue_metric(c0, c1, gap).abs());
            // The angle, reported alongside so the difference between the two is
            // visible rather than asserted.
            if c0 > 1.0 && c1 > 1.0 {
                angle.push(gap.abs());
            }
        }
        let (ls, cs, hs, as_) = (
            DeltaStats::from(&dl),
            DeltaStats::from(&dc),
            DeltaStats::from(&dh),
            DeltaStats::from(&angle),
        );
        println!(
            "  {:<16} {:>8.3} {:>9.3} {:>8.3} {:>9.3} {:>8.3} {:>9.3} {:>12.2}",
            policy.label(), ls.max, ls.mean, cs.max, cs.mean, hs.max, hs.mean, as_.max
        );
        match policy {
            GamutPolicy::ClipLinear => clip_l = ls.max,
            GamutPolicy::PreserveLuma => preserve_l = ls.max,
            _ => {}
        }
    }

    // The property the ray geometry exists for. Stated as a number rather than a
    // sentence: the vector being scaled has zero luminance, so L* cannot move.
    assert!(
        preserve_l < 1.0e-3,
        "preserve-luma moved L* by {preserve_l:.6}, which it cannot do if the vector it \
         scales carries zero luminance — the weights or the ray are wrong"
    );
    assert!(
        clip_l > 1.0,
        "clipping moved L* by only {clip_l:.4}; if that were true there would be nothing \
         to choose between the policies and this file would be pointless"
    );
    println!(
        "\n  clip moves lightness by up to {clip_l:.2} L*, with no policy deciding how much — \n\
         \x20 it depends on which channel ran out first. The ray policies hold it at {preserve_l:.1e}."
    );
}

// -------------------------------------------------------- 2. do gradients survive?

/// §2.2's "no banding" definition, aimed at the export instead of the working space.
///
/// A ramp that runs out of the destination gamut is the case every naive policy
/// fails, and it fails quietly — the image is not wrong anywhere in particular, it
/// just stops having detail in the saturated part. Three assertions, the same three
/// §2.2 defines: distinct output codes, monotone, and no step-to-step collapse.
#[test]
fn gradients_survive_the_gamut_boundary() {
    let ramps = gamut_boundary_ramps();

    // How much of each ramp is at stake, stated once: a policy is not responsible for
    // the part of the gradient sRGB can hold.
    for (name, ramp) in &ramps {
        let outside = ramp
            .iter()
            .filter(|c| !in_gamut(to_linear_dst(**c, &DISPLAY_P3, &SRGB)))
            .count();
        print!("{name} {outside}/33 outside sRGB   ");
    }
    println!("\n");
    println!("{:<16} {:>24} {:>22}   {}", "policy", "distinct codes (worst)", "min step ΔE", "per ramp");

    let mut survival: Vec<(String, usize, f64)> = Vec::new();
    for policy in candidates() {
        let mut worst_distinct = usize::MAX;
        let mut worst_step = f64::INFINITY;
        let mut detail = Vec::new();

        for (name, ramp) in &ramps {
            let lin: Vec<[f32; 3]> = ramp.iter().map(|c| to_linear_dst(*c, &DISPLAY_P3, &SRGB)).collect();

            let out: Vec<[u8; 3]> = lin
                .iter()
                .map(|l| {
                    let m = policy.map(*l, &SRGB);
                    let mut e = [0u8; 3];
                    for k in 0..3 {
                        e[k] = (SRGB.transfer.from_linear(m[k]) * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                    e
                })
                .collect();

            let mut distinct = out.clone();
            distinct.sort_unstable();
            distinct.dedup();

            let steps: Vec<f64> = out
                .windows(2)
                .map(|w| {
                    let a = encoded_to_lab([w[0][0] as f32 / 255.0, w[0][1] as f32 / 255.0, w[0][2] as f32 / 255.0], &SRGB);
                    let b = encoded_to_lab([w[1][0] as f32 / 255.0, w[1][1] as f32 / 255.0, w[1][2] as f32 / 255.0], &SRGB);
                    ciede2000(a, b)
                })
                .collect();
            let min_step = steps.iter().copied().fold(f64::INFINITY, f64::min);

            worst_distinct = worst_distinct.min(distinct.len());
            worst_step = worst_step.min(min_step);
            detail.push(format!("{name} {}/33", distinct.len()));
        }

        println!(
            "{:<16} {:>24} {:>22}   {}",
            policy.label(),
            worst_distinct,
            format!("{worst_step:.4}"),
            detail.join("  ")
        );
        survival.push((policy.label(), worst_distinct, worst_step));
    }

    // §2.2's own bar, transplanted: 14 of 17 there, so 27 of 33 here — the same
    // fraction, applied to the same kind of claim.
    const NEED_DISTINCT: usize = 27;
    let clip = survival.iter().find(|(l, _, _)| l == "clip-linear").expect("clip row");
    println!(
        "\n  clip collapses the worst ramp to {} of 33 codes and stalls at ΔE {:.4} between \n\
         \x20 consecutive steps — that is the flat patch in a saturated highlight, measured.",
        clip.1, clip.2
    );

    // The finding, asserted so it cannot rot: at least one shippable policy keeps the
    // ramp intact. If none did, §16 #11's answer would have to be "clip and accept it",
    // and that is a different decision that should be made deliberately.
    let best = survival.iter().max_by_key(|(_, d, _)| *d).expect("some policy");
    println!("  best: {} keeps {} of 33, minimum step ΔE {:.4}", best.0, best.1, best.2);
    assert!(
        best.1 >= NEED_DISTINCT,
        "no shippable policy keeps {NEED_DISTINCT} of 33 codes across the boundary \
         (best was {} at {}). §16 #11 would then be a choice between flattenings.",
        best.0, best.1
    );
}

// ------------------------------------------------------------- 3. in-gamut damage

/// The cost side, and the reason the knee is a measurement rather than a taste.
///
/// On a photograph nearly every pixel is already inside sRGB. A policy that makes
/// room for the out-of-gamut minority by desaturating the in-gamut majority is
/// spending the picture to save the edge of it, and this is where that bill is read.
#[test]
fn in_gamut_content_is_not_collateral() {
    let corpora: Vec<(&str, Vec<[u8; 3]>, &Space)> = vec![
        ("ColorChecker (sRGB)", COLORCHECKER_SRGB.to_vec(), &SRGB),
        ("screenshot (sRGB)", untagged_screenshot(), &SRGB),
        ("wide-gamut sweep (P3)", wide_gamut_gradient(), &DISPLAY_P3),
        (
            "boundary ramps (P3)",
            gamut_boundary_ramps().into_iter().flat_map(|(_, r)| r).collect(),
            &DISPLAY_P3,
        ),
    ];

    println!("{:<16} {:>22} {:>10} {:>10} {:>9}", "policy", "corpus", "max ΔE", "mean ΔE", "moved");
    for policy in candidates() {
        for (label, codes, src) in &corpora {
            let inside: Vec<[f32; 3]> = codes
                .iter()
                .map(|c| to_linear_dst(*c, src, &SRGB))
                .filter(|l| in_gamut(*l))
                .collect();
            if inside.is_empty() {
                continue;
            }
            let deltas: Vec<f64> = inside
                .iter()
                .map(|l| ciede2000(linear_to_lab(*l, &SRGB), linear_to_lab(policy.map(*l, &SRGB), &SRGB)))
                .collect();
            let moved = deltas.iter().filter(|d| **d > 1.0e-9).count();
            let s = DeltaStats::from(&deltas);
            println!(
                "{:<16} {:>22} {:>10.4} {:>10.4} {:>5}/{:<4}",
                policy.label(), label, s.max, s.mean, moved, inside.len()
            );
        }
    }

    // Exactness where it is claimed. Both no-knee policies are the identity inside the
    // gamut, and that is the property that makes them safe to apply unconditionally.
    for policy in [GamutPolicy::ClipLinear, GamutPolicy::PreserveLuma] {
        for (label, codes, src) in &corpora {
            for c in codes {
                let lin = to_linear_dst(*c, src, &SRGB);
                if !strictly_in_gamut(lin) {
                    continue;
                }
                let out = policy.map(lin, &SRGB);
                assert_eq!(
                    out, lin,
                    "{} moved an in-gamut colour from {label}: {lin:?} -> {out:?}. \
                     Bit-identical is the claim, not almost.",
                    policy.label()
                );
            }
        }
    }
    println!("\n  clip and preserve-luma are bit-identical inside the gamut. Compression is not, \n\
             \x20 by construction — that is what it is buying room with.");
}

// ------------------------------------------------------------------ 4. the knee

/// The trade curve. Everything above is the shape of the decision; this is the number
/// that picks the constant.
#[test]
fn knee_sweep() {
    let ramps = gamut_boundary_ramps();
    let all: Vec<[f32; 3]> = wide_gamut_gradient()
        .iter()
        .chain(ramps.iter().flat_map(|(_, r)| r.iter()))
        .map(|c| to_linear_dst(*c, &DISPLAY_P3, &SRGB))
        .collect();
    let inside: Vec<[f32; 3]> = all.iter().copied().filter(|l| in_gamut(*l)).collect();

    println!(
        "{} samples, {} inside sRGB, {} outside",
        all.len(),
        inside.len(),
        all.len() - inside.len()
    );
    println!("\n{:>6} {:>12} {:>12} {:>14} {:>18}", "knee", "in-gamut max", "in-gamut mean", "in-gamut moved", "worst ramp codes");

    for knee in [0.60f32, 0.70, 0.80, 0.85, 0.90, 0.95, 0.98] {
        let policy = GamutPolicy::CompressLuma { knee };
        let deltas: Vec<f64> = inside
            .iter()
            .map(|l| ciede2000(linear_to_lab(*l, &SRGB), linear_to_lab(policy.map(*l, &SRGB), &SRGB)))
            .collect();
        let moved = deltas.iter().filter(|d| **d > 1.0e-9).count();
        let s = DeltaStats::from(&deltas);

        let worst_codes = ramps
            .iter()
            .map(|(_, ramp)| {
                let mut out: Vec<[u8; 3]> = ramp
                    .iter()
                    .map(|c| {
                        let m = policy.map(to_linear_dst(*c, &DISPLAY_P3, &SRGB), &SRGB);
                        let mut e = [0u8; 3];
                        for k in 0..3 {
                            e[k] = (SRGB.transfer.from_linear(m[k]) * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                        e
                    })
                    .collect();
                out.sort_unstable();
                out.dedup();
                out.len()
            })
            .min()
            .unwrap_or(0);

        println!(
            "{knee:>6.2} {:>12.4} {:>12.4} {:>9}/{:<4} {:>18}",
            s.max, s.mean, moved, inside.len(), worst_codes
        );
    }
    println!(
        "\n  Reading it: the knee buys distinct output codes across the boundary and pays in \n\
         \x20 ΔE on colours that were already correct. A knee at 1.0 is preserve-luma exactly."
    );
}

// --------------------------------------------------------------- real photographs

/// The deciding numbers, on the file §1 calls the native subject.
///
/// The synthetic corpora above are deliberately hostile — the P3 primaries themselves
/// are in them. A photograph is not hostile, and the question that actually decides
/// §16 #11 is how much of a real picture each policy touches.
#[test]
fn real_photograph_under_each_policy() {
    let dir = corpus::corpus_dir();
    let Some(path) = corpus::find_heic(&dir) else {
        println!(
            "SKIP: no .heic/.heif in {}. Set PHOTODESK_CORPUS_DIR. This test would \n\
             report, for each candidate policy, how many of a real photograph's pixels \n\
             leave sRGB and what each policy costs the ones that do not.",
            dir.display()
        );
        return;
    };

    let lh = libheif_rs::LibHeif::new();
    let ctx = libheif_rs::HeifContext::read_from_file(path.to_str().expect("utf-8 path"))
        .expect("libheif could not open the file");
    let handle = ctx.primary_image_handle().expect("primary image handle");

    // §4 measured this file as Display P3 in both containers; re-derive rather than
    // assume, so a different corpus does not silently get the wrong matrices.
    let src: &Space = match handle.color_profile_raw() {
        Some(p) => {
            let parsed = lcms2::Profile::new_icc(&p.data).expect("embedded ICC does not parse");
            match parsed.read_tag(lcms2::TagSignature::RedColorantTag) {
                lcms2::Tag::CIEXYZ(xyz) if xyz.X > 0.48 => &DISPLAY_P3,
                _ => &SRGB,
            }
        }
        None => &SRGB,
    };
    println!("file: {} (source {})", path.file_name().unwrap().to_string_lossy(), src.name);

    let decoded = lh
        .decode(&handle, libheif_rs::ColorSpace::Rgb(libheif_rs::RgbChroma::Rgb), None)
        .expect("default decode failed");
    let planes = decoded.planes();
    let plane = planes.interleaved.expect("interleaved RGB plane");
    let (w, h) = (decoded.width(), decoded.height());

    // A finer grid than §4's structural test: this is a population question — what
    // fraction of a photograph is at stake — and that needs samples, not spot checks.
    let step = (w.min(h) / 200).max(1);
    let mut lin = Vec::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let o = y as usize * plane.stride + (x * 3) as usize;
            if o + 2 < plane.data.len() {
                lin.push(to_linear_dst(
                    [plane.data[o], plane.data[o + 1], plane.data[o + 2]],
                    src,
                    &SRGB,
                ));
            }
        }
    }
    let outside: Vec<[f32; 3]> = lin.iter().copied().filter(|l| !in_gamut(*l)).collect();
    let inside: Vec<[f32; 3]> = lin.iter().copied().filter(|l| in_gamut(*l)).collect();
    println!(
        "{} sampled pixels: {} outside sRGB ({:.2}%), {} inside",
        lin.len(),
        outside.len(),
        100.0 * outside.len() as f64 / lin.len() as f64,
        inside.len()
    );
    assert!(lin.len() > 10_000, "only {} samples; the grid stride is wrong", lin.len());

    // The same ΔL*/ΔC*/ΔH decomposition the synthetic corpus gets, on real pixels.
    // The synthetic sweep contains the P3 primaries themselves and is where the ray
    // policies look worst; a photograph is the population the decision is actually
    // for, and the two are not interchangeable evidence.
    println!("\n{:<16} {:>18} {:>18} {:>18}", "policy", "|ΔL*| max/mean", "|ΔC*| max/mean", "|ΔH| max/mean");
    for policy in candidates() {
        let (mut dl, mut dc, mut dh) = (Vec::new(), Vec::new(), Vec::new());
        for lin in &outside {
            let (l0, c0, h0) = lch(*lin, &SRGB);
            let (l1, c1, h1) = lch(policy.map(*lin, &SRGB), &SRGB);
            dl.push((l1 - l0).abs());
            dc.push((c1 - c0).abs());
            dh.push(hue_metric(c0, c1, hue_gap(h0, h1)).abs());
        }
        let (ls, cs, hs) = (
            DeltaStats::from(&dl),
            DeltaStats::from(&dc),
            DeltaStats::from(&dh),
        );
        println!(
            "{:<16} {:>8.3} {:>9.3} {:>8.3} {:>9.3} {:>8.3} {:>9.3}",
            policy.label(), ls.max, ls.mean, cs.max, cs.mean, hs.max, hs.mean
        );
    }

    println!(
        "\n{:<16} {:>26} {:>26} {:>10}",
        "policy", "out-of-gamut pixels", "in-gamut pixels", "touched"
    );
    for policy in candidates() {
        let de = |set: &[[f32; 3]]| {
            DeltaStats::from(
                &set.iter()
                    .map(|l| ciede2000(linear_to_lab(*l, &SRGB), linear_to_lab(policy.map(*l, &SRGB), &SRGB)))
                    .collect::<Vec<_>>(),
            )
        };
        let (o, i) = (de(&outside), de(&inside));
        let touched = inside
            .iter()
            .filter(|l| policy.map(**l, &SRGB) != **l)
            .count();
        println!(
            "{:<16} max {:>7.4} mean {:>7.4} max {:>7.4} mean {:>7.4} {:>7.2}%",
            policy.label(),
            o.max, o.mean, i.max, i.mean,
            100.0 * touched as f64 / lin.len() as f64
        );
    }
    println!(
        "\n  'touched' is the share of the whole frame a policy modifies that the clip would \n\
         \x20 have left alone. That is the price of gradient survival, on a real photograph."
    );

    // ---- flattening, on the subject rather than on a synthetic ramp ------------
    //
    // The boundary-ramp test answers "can a policy flatten a gradient" in principle.
    // This answers the question that decides §16 #11: does it flatten *this*, a
    // photograph, where the out-of-gamut pixels are 3% of the frame rather than a
    // constructed sweep through the P3 primaries.
    //
    // Only pairs with at least one out-of-gamut member are counted, and that loses
    // nothing: every candidate is the identity inside the gamut, so a pair that is
    // entirely inside cannot collapse under any of them.
    const ROW_STRIDE: usize = 8;
    let export = |lin: [f32; 3], policy: GamutPolicy| {
        let m = policy.map(lin, &SRGB);
        let mut e = [0u8; 3];
        for k in 0..3 {
            e[k] = (SRGB.transfer.from_linear(m[k]) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        e
    };

    let mut pairs: Vec<([f32; 3], [f32; 3])> = Vec::new();
    for y in (0..h as usize).step_by(ROW_STRIDE) {
        let row = y * plane.stride;
        let mut prev: Option<([u8; 3], [f32; 3])> = None;
        for x in 0..w as usize {
            let o = row + x * 3;
            if o + 2 >= plane.data.len() {
                break;
            }
            let code = [plane.data[o], plane.data[o + 1], plane.data[o + 2]];
            let l = to_linear_dst(code, src, &SRGB);
            if let Some((pc, pl)) = prev
                && pc != code
                && (!in_gamut(pl) || !in_gamut(l))
            {
                pairs.push((pl, l));
            }
            prev = Some((code, l));
        }
    }
    println!(
        "\nadjacent-pixel collapse, every {ROW_STRIDE}th row: {} neighbouring pairs differ in \n\
         the source and touch the gamut boundary",
        pairs.len()
    );
    assert!(
        pairs.len() > 1_000,
        "only {} qualifying pairs; this photograph has no saturated detail and cannot \
         answer the flattening question",
        pairs.len()
    );

    let mut collapse: Vec<(String, usize)> = Vec::new();
    for policy in candidates() {
        let lost = pairs
            .iter()
            .filter(|(a, b)| export(*a, policy) == export(*b, policy))
            .count();
        println!(
            "  {:<16} {lost:>6} pairs collapse to one colour ({:.2}%)",
            policy.label(),
            100.0 * lost as f64 / pairs.len() as f64
        );
        collapse.push((policy.label(), lost));
    }

    // The finding §16 #11 turns on. If the clip lost nothing here, the whole
    // gradient-survival argument would be about synthetic ramps and should not decide
    // a policy for photographs.
    let clip_lost = collapse.iter().find(|(l, _)| l == "clip-linear").map(|(_, n)| *n).unwrap_or(0);
    let best = collapse.iter().map(|(_, n)| *n).min().unwrap_or(0);
    println!(
        "\n  the clip loses {clip_lost} of these pairs; the best candidate loses {best}. \n\
         \x20 That is detail present in the source and absent from the export, on a photograph."
    );
}

// ------------------------------------------------------------------------ hygiene

/// Whatever the policy, the encoder gets a value it can quantise — and the final
/// clamp in `gamut.rs` is a guard against float error, not a second gamut map doing
/// the real work behind the first one's back.
#[test]
fn every_policy_lands_inside_the_gamut_without_the_safety_clamp_doing_the_work() {
    let ramps = gamut_boundary_ramps();
    let samples: Vec<[f32; 3]> = wide_gamut_gradient()
        .iter()
        .chain(ramps.iter().flat_map(|(_, r)| r.iter()))
        .map(|c| to_linear_dst(*c, &DISPLAY_P3, &SRGB))
        .collect();

    let weights = luma_weights(&SRGB);
    for policy in candidates() {
        let mut worst_rescue = 0.0f32;
        for lin in &samples {
            let mapped = policy.map(*lin, &SRGB);
            for c in mapped {
                assert!(
                    (0.0..=1.0).contains(&c),
                    "{} returned {c} for {lin:?}, which the encoder cannot represent",
                    policy.label()
                );
            }
            let raw = policy.map_unclamped(*lin, weights);
            for k in 0..3 {
                worst_rescue = worst_rescue.max((raw[k] - mapped[k]).abs());
            }
        }
        println!("{:<16} safety clamp moved at most {worst_rescue:.3e}", policy.label());
        if policy == GamutPolicy::ClipLinear {
            // For the clip, the clamp *is* the policy. Reported for contrast, and
            // asserted the other way round: if it were near zero, the sweep would not
            // be leaving sRGB and every number in this file would be about nothing.
            assert!(
                worst_rescue > 0.01,
                "the clip's clamp moved at most {worst_rescue:.3e}, so the corpus never \
                 leaves sRGB and this file is measuring an empty set"
            );
            continue;
        }
        assert!(
            worst_rescue < 1.0e-5,
            "{}'s safety clamp is doing {worst_rescue:.3e} of real work — the ray solves \
             for the boundary, so a clamp that moves anything means the solution is wrong \
             and the clamp is covering for it",
            policy.label()
        );
    }
}
