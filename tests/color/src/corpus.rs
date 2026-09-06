//! The §2.2 corpus.
//!
//! Four of the six items are synthetic and buildable with nothing installed. The two
//! that are not — a real iPhone HEIF with an embedded P3 profile, and an iPhone HEIC
//! carrying an ISO gain map — needed `libheif-devel` and a real photograph, and both
//! arrived on 2026-09-06. [`BLOCKED`] is the mechanism that made the gap visible in
//! code rather than only in a document; it is kept, and empty.

use photodesk::engine::colour::{DISPLAY_P3, SRGB, Space, Transfer};

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

/// Ramps that start inside sRGB and run out of it — the corpus item the *export*
/// gamut policy is judged on (§16 #11).
///
/// §2.2's deep-shadow ramp asks whether the working space can carry sixteen adjacent
/// codes without collapsing them. This asks the same question of the other end of the
/// chain. A gradient that runs off the edge of the destination gamut is where a naive
/// export policy shows itself, and it is the shape real content has: a sunset, a
/// backlit petal, a saturated sky. Encoded Display P3 values, because that is what a
/// file actually holds — a gradient in a photograph is a gradient in encoded codes,
/// not in linear light.
///
/// Each ramp runs from mid grey to a fully saturated Display P3 corner, so it is
/// inside sRGB at one end and well outside at the other, and the crossing happens
/// somewhere in the middle where a policy has to make a choice per step rather than
/// once.
pub fn gamut_boundary_ramps() -> Vec<(&'static str, Vec<[u8; 3]>)> {
    const STEPS: i32 = 32;
    const TARGETS: [(&str, [i32; 3]); 6] = [
        ("red", [255, 0, 0]),
        ("green", [0, 255, 0]),
        ("blue", [0, 0, 255]),
        ("yellow", [255, 255, 0]),
        ("cyan", [0, 255, 255]),
        ("magenta", [255, 0, 255]),
    ];

    TARGETS
        .iter()
        .map(|(name, target)| {
            let ramp = (0..=STEPS)
                .map(|i| {
                    let t = i as f32 / STEPS as f32;
                    let mut out = [0u8; 3];
                    for (k, cell) in out.iter_mut().enumerate() {
                        let from = 128.0;
                        *cell = (from + t * (target[k] as f32 - from)).round().clamp(0.0, 255.0)
                            as u8;
                    }
                    out
                })
                .collect();
            (*name, ramp)
        })
        .collect()
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

/// Corpus items that cannot be built on this machine, stated rather than quietly
/// omitted.
///
/// **Empty, and kept.** It held two entries — a real P3-tagged iPhone HEIF and a
/// gain-mapped HEIC — from the day Spike B was written until 2026-09-06, when
/// `libheif-devel` and real photographs both arrived and `heif_icc.rs` and
/// `real_photos.rs` closed them (`DECISIONS.md`). The mechanism stays because the
/// next blocked item should land in code where a test prints it, not in a document
/// where it is somebody's job to remember it.
///
/// Note what this list is *not* for: `real_photos.rs` skipping because the corpus
/// directory is empty is a different thing entirely — the item exists and can be
/// built, this machine just has no photograph in front of it today.
pub const BLOCKED: [Unavailable; 0] = [];

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


// ------------------------------------------------------------- real photographs

/// Where the real-photograph corpus lives.
///
/// **The photographs are not in the repository and must not be.** They are personal
/// files; §12.1 puts corpus binaries behind git-lfs; and a test that only runs where
/// the data is happens to be the honest arrangement. Point `PHOTODESK_CORPUS_DIR` at
/// a directory of real photographs, or drop them in `~/Downloads`.
///
/// Lives here rather than in one of the test files because two of them need it now,
/// and a second copy of "where the photographs are" is a second thing to keep in step.
pub fn corpus_dir() -> std::path::PathBuf {
    std::env::var("PHOTODESK_CORPUS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads")
        })
}

fn find_by_extension(dir: &std::path::Path, exts: &[&str]) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.iter().any(|w| e.eq_ignore_ascii_case(w)))
        })
        .collect();
    found.sort();
    found
}

/// The first HEIC/HEIF in the corpus directory, in name order so runs are repeatable.
pub fn find_heic(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    find_by_extension(dir, &["heic", "heif"]).into_iter().next()
}

/// The largest JPEG in the corpus directory — a camera original rather than a
/// downloaded thumbnail.
pub fn find_jpeg(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut found = find_by_extension(dir, &["jpg", "jpeg"]);
    found.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));
    found.pop()
}
