//! CIEDE2000.
//!
//! Every threshold in §2.2 is stated in ΔE2000, so an error in this file would make
//! the whole harness agree with itself and be wrong. It is cross-validated against
//! lcms2 over random Lab pairs in `tests/spike_b.rs` rather than trusted.

/// CIEDE2000 with k_L = k_C = k_H = 1.
pub fn ciede2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    let (l1, a1, b1) = (lab1[0], lab1[1], lab1[2]);
    let (l2, a2, b2) = (lab2[0], lab2[1], lab2[2]);

    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let c_bar = (c1 + c2) / 2.0;

    let c_bar7 = c_bar.powi(7);
    const P25_7: f64 = 6_103_515_625.0; // 25^7
    let g = 0.5 * (1.0 - (c_bar7 / (c_bar7 + P25_7)).sqrt());

    let a1p = (1.0 + g) * a1;
    let a2p = (1.0 + g) * a2;
    let c1p = (a1p * a1p + b1 * b1).sqrt();
    let c2p = (a2p * a2p + b2 * b2).sqrt();

    let h1p = hue_deg(b1, a1p);
    let h2p = hue_deg(b2, a2p);

    let dlp = l2 - l1;
    let dcp = c2p - c1p;

    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d.abs() <= 180.0 {
            d
        } else if d > 180.0 {
            d - 360.0
        } else {
            d + 360.0
        }
    };
    let dhp_big = 2.0 * (c1p * c2p).sqrt() * (dhp.to_radians() / 2.0).sin();

    let lp_bar = (l1 + l2) / 2.0;
    let cp_bar = (c1p + c2p) / 2.0;

    let hp_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else {
        let d = (h1p - h2p).abs();
        if d <= 180.0 {
            (h1p + h2p) / 2.0
        } else if h1p + h2p < 360.0 {
            (h1p + h2p + 360.0) / 2.0
        } else {
            (h1p + h2p - 360.0) / 2.0
        }
    };

    let t = 1.0 - 0.17 * (hp_bar - 30.0).to_radians().cos()
        + 0.24 * (2.0 * hp_bar).to_radians().cos()
        + 0.32 * (3.0 * hp_bar + 6.0).to_radians().cos()
        - 0.20 * (4.0 * hp_bar - 63.0).to_radians().cos();

    let d_theta = 30.0 * (-(((hp_bar - 275.0) / 25.0).powi(2))).exp();
    let cp_bar7 = cp_bar.powi(7);
    let rc = 2.0 * (cp_bar7 / (cp_bar7 + P25_7)).sqrt();

    let sl = 1.0 + (0.015 * (lp_bar - 50.0).powi(2)) / (20.0 + (lp_bar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let rt = -(2.0 * d_theta).to_radians().sin() * rc;

    let term_l = dlp / sl;
    let term_c = dcp / sc;
    let term_h = dhp_big / sh;

    (term_l * term_l + term_c * term_c + term_h * term_h + rt * term_c * term_h).sqrt()
}

/// atan2 in degrees, wrapped to [0, 360). Zero for an achromatic sample, which is
/// the convention CIEDE2000 requires — the hue terms are gated off in that case anyway.
fn hue_deg(b: f64, ap: f64) -> f64 {
    if b == 0.0 && ap == 0.0 {
        return 0.0;
    }
    let deg = b.atan2(ap).to_degrees();
    if deg < 0.0 { deg + 360.0 } else { deg }
}

/// Summary of a ΔE distribution over a corpus. Reported rather than just asserted,
/// because "passed" without a number tells the next person nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeltaStats {
    pub count: usize,
    pub max: f64,
    pub mean: f64,
    pub p95: f64,
}

impl DeltaStats {
    pub fn from(deltas: &[f64]) -> Self {
        if deltas.is_empty() {
            return Self::default();
        }
        let mut sorted = deltas.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("NaN in delta set"));
        let idx = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        Self {
            count: sorted.len(),
            max: *sorted.last().unwrap(),
            mean: sorted.iter().sum::<f64>() / sorted.len() as f64,
            p95: sorted[idx],
        }
    }
}

impl std::fmt::Display for DeltaStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={} max={:.4} mean={:.4} p95={:.4}",
            self.count, self.max, self.mean, self.p95
        )
    }
}
