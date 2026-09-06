//! CIE L*a*b*, and the gamut control that needs it.
//!
//! **Measurement, not transform**, which is why it is here and not in the product.
//! `engine::colour` carries the spaces and matrices a photograph actually goes
//! through; Lab exists so this harness can say how far two colours are apart, and §1's
//! non-goals are emphatic that no part of that becomes a feature. Shipping unused
//! colour science invites somebody to use it.
//!
//! The nearest in-gamut colour under ΔE2000 — **a control, not a candidate.**

use photodesk::engine::colour::Space;
use photodesk::engine::gamut::{GamutPolicy, luma_weights};

use crate::delta_e::ciede2000;

/// CIE XYZ (D65-relative, Y in [0,1]) -> CIE L*a*b*.
pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let w = crate::lab::d65_xyz();
    let f = |t: f64| {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA * DELTA * DELTA {
            t.cbrt()
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };
    let fx = f(xyz[0] / w[0]);
    let fy = f(xyz[1] / w[1]);
    let fz = f(xyz[2] / w[2]);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Encoded RGB in `space` -> CIE L*a*b*, for measurement only.
pub fn encoded_to_lab(rgb: [f32; 3], space: &Space) -> [f64; 3] {
    linear_to_lab(
        [
            space.transfer.to_linear(rgb[0]),
            space.transfer.to_linear(rgb[1]),
            space.transfer.to_linear(rgb[2]),
        ],
        space,
    )
}

/// Linear RGB in `space` -> CIE L*a*b*, for measurement only.
///
/// The gamut policies in `gamut.rs` work in linear destination RGB and have to be
/// measured there — encoding first would fold the transfer curve's own clamping into
/// a number that is supposed to be about the gamut. Deliberately *not* clamped: a
/// channel outside [0,1] has a perfectly good Lab coordinate, and losing it here
/// would make an out-of-gamut colour indistinguishable from its own clipped version
/// inside the very comparison that exists to tell them apart.
pub fn linear_to_lab(lin: [f32; 3], space: &Space) -> [f64; 3] {
    let m = space.to_xyz();
    let xyz = [
        m.0[0][0] * lin[0] as f64 + m.0[0][1] * lin[1] as f64 + m.0[0][2] * lin[2] as f64,
        m.0[1][0] * lin[0] as f64 + m.0[1][1] * lin[1] as f64 + m.0[1][2] * lin[2] as f64,
        m.0[2][0] * lin[0] as f64 + m.0[2][1] * lin[1] as f64 + m.0[2][2] * lin[2] as f64,
    ];
    xyz_to_lab(xyz)
}

/// D65's XYZ, normalised to Y = 1. Derived here rather than imported because
/// `Xy::to_xyz` is private to the product — the white point is the product's business
/// and this is the measurement's copy of one number.
fn d65_xyz() -> [f64; 3] {
    let d65 = photodesk::engine::colour::D65;
    [d65.x / d65.y, 1.0, (1.0 - d65.x - d65.y) / d65.y]
}

/// The nearest in-gamut colour under ΔE2000 — **a control, not a candidate.**
///
/// It answers the question that decides how to read every other number here: if the
/// goal were "move the colour as little as possible", what would win? The answer is a
/// per-pixel search over the destination cube, which is not a thing a fragment shader
/// can do at 60 fps and not a thing anyone would want if it could — it flattens
/// gradients as thoroughly as the clip does, because the nearest in-gamut colour to
/// everything outside the gamut is on the boundary.
///
/// Local search by coordinate descent from the clip, with a shrinking step. It can in
/// principle stop short of the global optimum; that costs nothing here, because its
/// job is to *beat the shippable policies on ΔE*, and a local minimum that already
/// does so makes the point a global one could only make harder.
pub fn nearest_in_gamut_lab(c: [f32; 3], dst: &Space) -> [f32; 3] {
    let target = linear_to_lab(c, dst);
    let mut best = GamutPolicy::ClipLinear.map_with(c, luma_weights(dst));
    let mut best_d = ciede2000(target, linear_to_lab(best, dst));

    let mut step = 0.25f32;
    while step > 1.0e-4 {
        let mut improved = false;
        for axis in 0..3 {
            for dir in [-1.0f32, 1.0] {
                let mut candidate = best;
                candidate[axis] = (candidate[axis] + dir * step).clamp(0.0, 1.0);
                let d = ciede2000(target, linear_to_lab(candidate, dst));
                if d < best_d - 1.0e-12 {
                    best_d = d;
                    best = candidate;
                    improved = true;
                }
            }
        }
        if !improved {
            step *= 0.5;
        }
    }
    best
}
