//! The §2.2 corpus.
//!
//! Four of the six items are synthetic and buildable with nothing installed. The two
//! that are not — a real iPhone HEIF with an embedded P3 profile, and an iPhone HEIC
//! carrying an ISO gain map — need `libheif-devel`, which is not present on this
//! machine. They are declared here so the gap is visible in code rather than only in
//! a document, and so the day the package lands the corpus is one function away.

use crate::colour::{DISPLAY_P3, SRGB, Space, Transfer};

/// The 24 ColorChecker patches as 8-bit sRGB (BabelColor average values).
///
/// Ground truth for the harness. Stored in sRGB only: the Display P3 encoding of the
/// same colours is *derived* by [`reference_convert`] rather than written down, so a
/// second table cannot drift from the first.
pub const COLORCHECKER_SRGB: [[u8; 3]; 24] = [
    [115, 82, 68],    // dark skin
    [194, 150, 130],  // light skin
    [98, 122, 157],   // blue sky
    [87, 108, 67],    // foliage
    [133, 128, 177],  // blue flower
    [103, 189, 170],  // bluish green
    [214, 126, 44],   // orange
    [80, 91, 166],    // purplish blue
    [193, 90, 99],    // moderate red
    [94, 60, 108],    // purple
    [157, 188, 64],   // yellow green
    [224, 163, 46],   // orange yellow
    [56, 61, 150],    // blue
    [70, 148, 73],    // green
    [175, 54, 60],    // red
    [231, 199, 31],   // yellow
    [187, 86, 149],   // magenta
    [8, 133, 161],    // cyan
    [243, 243, 242],  // white
    [200, 200, 200],  // neutral 8
    [160, 160, 160],  // neutral 6.5
    [122, 122, 121],  // neutral 5
    [85, 85, 85],     // neutral 3.5
    [52, 52, 52],     // black
];

/// A screenshot with no embedded profile. §4 says assume sRGB.
///
/// Chosen to be the kind of thing a screenshot actually contains — flat UI greys,
/// saturated accent colours, pure black and white — rather than photographic tones,
/// because a misinterpreted profile shows up worst on flat synthetic colour.
pub fn untagged_screenshot() -> Vec<[u8; 3]> {
    let mut v = vec![
        [0, 0, 0],
        [255, 255, 255],
        [30, 30, 30],
        [46, 52, 64],
        [216, 222, 233],
        [191, 97, 106],
        [163, 190, 140],
        [235, 203, 139],
        [129, 161, 193],
        [180, 142, 173],
    ];
    // A grey staircase, where a wrong transfer function is most legible.
    for i in 0..=16 {
        let c = (i * 16).min(255) as u8;
        v.push([c, c, c]);
    }
    v
}

/// A wide-gamut sweep: saturated Display P3 encodings, most of which fall outside sRGB.
///
/// This is the corpus item that exercises the export gamut policy rather than the
/// working space, and it is deliberately hostile — the primaries themselves are in it.
pub fn wide_gamut_gradient() -> Vec<[u8; 3]> {
    let mut v = Vec::new();
    for step in 0..=16 {
        let t = (step as f32 / 16.0 * 255.0).round() as u8;
        v.push([t, 0, 0]);
        v.push([0, t, 0]);
        v.push([0, 0, t]);
        v.push([t, t, 0]);
        v.push([0, t, t]);
        v.push([t, 0, t]);
    }
    v
}

/// §2.2's deep-shadow ramp: codes 0 through 16 inclusive, 17 steps.
///
/// The item the spike exists for. Half float has ~11 bits of significand and linear
/// encoding spends them in the highlights, so if f16 is going to fail it fails here.
pub fn deep_shadow_ramp() -> Vec<u8> {
    (0u8..=16).collect()
}

/// A corpus item that needs a decoder this machine does not have yet.
#[derive(Clone, Copy, Debug)]
pub struct Unavailable {
    pub item: &'static str,
    pub needs: &'static str,
    pub why_it_matters: &'static str,
}

/// The two §2.2 corpus items that are blocked, stated rather than quietly omitted.
pub const BLOCKED: [Unavailable; 2] = [
    Unavailable {
        item: "iPhone HEIF with an embedded Display P3 profile",
        needs: "libheif-devel (runtime libheif.so.1.21.2 is present; no pkg-config .pc)",
        why_it_matters: "The only corpus item that tests ICC extraction from a real \
                         container. Synthetic patches prove the matrices; they cannot \
                         prove we read the profile that says which matrices to use.",
    },
    Unavailable {
        item: "iPhone HEIC carrying an ISO HDR gain map",
        needs: "libheif-devel, and a gain-map-aware decode path",
        why_it_matters: "§4 discards the gain map in v1 deliberately. The test that \
                         matters is that the SDR base decodes correctly and the gain \
                         map is ignored rather than misapplied — which is a different \
                         assertion from 'we do not support it'.",
    },
];

/// High-precision reference conversion between two encoded spaces, in f64 throughout.
///
/// Deliberately *not* the pipeline: no f16, no f32 working buffer, no pass count.
/// It is the yardstick the pipeline is measured against, and it is itself measured
/// against lcms2 in `tests/spike_b.rs`. Two links, both tested.
pub fn reference_convert(encoded: [f64; 3], src: &Space, dst: &Space) -> [f64; 3] {
    let lin = [
        ref_to_linear(encoded[0], src.transfer),
        ref_to_linear(encoded[1], src.transfer),
        ref_to_linear(encoded[2], src.transfer),
    ];
    let m = dst.from_xyz().mul(&src.to_xyz()).0;
    let out = [
        m[0][0] * lin[0] + m[0][1] * lin[1] + m[0][2] * lin[2],
        m[1][0] * lin[0] + m[1][1] * lin[1] + m[1][2] * lin[2],
        m[2][0] * lin[0] + m[2][1] * lin[1] + m[2][2] * lin[2],
    ];
    [
        ref_from_linear(out[0].clamp(0.0, 1.0), dst.transfer),
        ref_from_linear(out[1].clamp(0.0, 1.0), dst.transfer),
        ref_from_linear(out[2].clamp(0.0, 1.0), dst.transfer),
    ]
}

fn ref_to_linear(v: f64, t: Transfer) -> f64 {
    match t {
        Transfer::Linear => v,
        Transfer::Srgb => {
            if v <= 0.040_449_936 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
    }
}

fn ref_from_linear(v: f64, t: Transfer) -> f64 {
    match t {
        Transfer::Linear => v,
        Transfer::Srgb => {
            if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        }
    }
}

/// The ColorChecker patches re-encoded as Display P3, derived rather than tabulated.
pub fn colorchecker_display_p3() -> Vec<[f64; 3]> {
    COLORCHECKER_SRGB
        .iter()
        .map(|c| {
            let e = [
                c[0] as f64 / 255.0,
                c[1] as f64 / 255.0,
                c[2] as f64 / 255.0,
            ];
            reference_convert(e, &SRGB, &DISPLAY_P3)
        })
        .collect()
}
