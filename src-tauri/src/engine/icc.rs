//! Reading an embedded ICC profile, and deciding which of §4's two spaces it is.
//!
//! §4 handles **two** spaces — sRGB and Display P3 — and the register says a third is
//! "a decision, not a value". So this module's job is not to interpret an arbitrary
//! profile; it is to answer *which of my two is this, and is it neither?* The last
//! case has to be an error rather than a guess, because `real_photos.rs` already put
//! the reason plainly: a profile we can parse but not classify is worse than no
//! profile, since the app would proceed confidently down the wrong path.
//!
//! ## Why this is hand-written rather than lcms2
//!
//! lcms2 is a dev-dependency of the colour harness and could have been one here. It is
//! not, for the same reason the harness derives its matrices from chromaticities
//! instead of tabulating them: the thing that decides how a photograph is interpreted
//! should be code this project can read, and the harness should be able to disagree
//! with it. `tests/color/` cross-checks this parser against lcms2 over the same real
//! profiles — two links, both tested, which is the arrangement §2.2 already uses for
//! ΔE2000 and the matrices.
//!
//! It also keeps a C library off the shipping path for something that is eighty lines
//! of byte reading.
//!
//! ## The part that is easy to get wrong
//!
//! **ICC colorants are stored in the profile connection space, which is D50** — not
//! in the profile's own white point. A Display P3 profile's `rXYZ` reads (0.5151,
//! 0.2412, −0.0011), while Display P3's red primary at D65 is (0.4866, 0.2290,
//! 0.0000). Comparing the tag against a D65-derived matrix would find neither space
//! and reject every photograph the application exists to open. So the colorants are
//! adapted D50 → D65 with Bradford before anything is compared.

use super::colour::{DISPLAY_P3, Mat3, SRGB, Space};

/// The ICC profile connection space illuminant, fixed by the specification as these
/// exact XYZ values rather than derived from a chromaticity.
///
/// Written down because it is a *definition* — ICC.1:2010 §7.2.16 — in the same way
/// the sRGB transfer curve's coefficients are. The project's "no tabulated constants"
/// rule is about values that can be derived and were copied instead; this one cannot.
const PCS_D50: [f64; 3] = [0.9642, 1.0000, 0.8249];

/// The Bradford cone response matrix, likewise a definition of the transform rather
/// than a derived quantity. Its inverse is computed rather than written down.
const BRADFORD: Mat3 = Mat3([
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
]);

/// How far a profile's adapted colorants may sit from a space's own before it is not
/// that space.
///
/// **Derived, not chosen.** The largest component gap between any two of the spaces
/// worth telling apart is what sets the scale: sRGB to Display P3 is 0.0934, and
/// Display P3 to Adobe RGB — the nearest common space this application does *not*
/// handle — is 0.0901. At 0.02 a profile has to be less than a quarter of the way from
/// Display P3 to Adobe RGB to be accepted as Display P3, while the quantisation of the
/// tags themselves (s15Fixed16, ~1.5 × 10⁻⁵) and the adaptation round trip (~10⁻⁴) are
/// three orders of magnitude below it. A threshold on the red colorant alone would
/// have read Adobe RGB as Display P3, which is a silent wrong-colour bug on a profile
/// people actually have.
const COLORANT_TOLERANCE: f64 = 0.02;

/// How far the profile's tone curve may sit from the sRGB curve.
///
/// Both of §4's spaces use it — Display P3 uses the sRGB curve, not DCI's 2.6 gamma,
/// and `colour.rs` already notes that getting that wrong is a ~4 ΔE error looking like
/// a gamut problem. A profile with our primaries and a 1.8 gamma is not one of our
/// spaces, and would be visibly wrong rather than subtly so.
const CURVE_TOLERANCE: f64 = 0.01;

#[derive(Clone, Debug, PartialEq)]
pub enum IccError {
    /// Not a profile: too short, or no `acsp` signature.
    NotAProfile(String),
    /// A profile, but missing something §4 needs to interpret it.
    Incomplete(String),
    /// Structurally fine and not one of the two spaces §4 handles.
    ///
    /// Carries what it found, because "unsupported profile" with no numbers is a
    /// message nobody can act on.
    UnknownSpace {
        red: [f64; 3],
        green: [f64; 3],
        blue: [f64; 3],
        nearest: &'static str,
        distance: f64,
    },
    /// Our primaries, somebody else's tone curve.
    UnknownCurve { space: &'static str, worst: f64 },
}

impl std::fmt::Display for IccError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IccError::NotAProfile(e) => write!(f, "not an ICC profile: {e}"),
            IccError::Incomplete(e) => write!(f, "ICC profile is missing {e}"),
            IccError::UnknownSpace { red, green, blue, nearest, distance } => write!(
                f,
                "this profile is not one PhotoDesk handles. Its primaries at D65 are \
                 R{red:.4?} G{green:.4?} B{blue:.4?}; the nearest space it knows is \
                 {nearest}, {distance:.4} away against a tolerance of {COLORANT_TOLERANCE}. \
                 §4 handles sRGB and Display P3, and reading a third as one of them \
                 would be a wrong colour rather than a refused file"
            ),
            IccError::UnknownCurve { space, worst } => write!(
                f,
                "this profile has {space}'s primaries but not its tone curve — it \
                 diverges by {worst:.4} against a tolerance of {CURVE_TOLERANCE}. §4's \
                 two spaces both use the sRGB curve"
            ),
        }
    }
}

impl std::error::Error for IccError {}

/// One channel's tone reproduction curve, in whichever of ICC's forms it was stored.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// `curv` with no entries: the identity.
    Identity,
    /// `curv` with one entry: a pure gamma, stored as u8Fixed8.
    Gamma(f64),
    /// `curv` with n entries: a sampled table, u16 normalised, linearly interpolated.
    Table(Vec<f64>),
    /// `para`, one of ICC's five parametric forms. Type 3 and 4 are the sRGB shape.
    Parametric { kind: u16, g: f64, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64 },
}

impl Curve {
    /// Encoded [0,1] → linear. The direction a decoder needs.
    pub fn to_linear(&self, x: f64) -> f64 {
        match self {
            Curve::Identity => x,
            Curve::Gamma(g) => x.powf(*g),
            Curve::Table(table) => {
                if table.len() < 2 {
                    return x;
                }
                let t = x.clamp(0.0, 1.0) * (table.len() - 1) as f64;
                let i = t.floor() as usize;
                let frac = t - i as f64;
                if i + 1 >= table.len() {
                    return table[table.len() - 1];
                }
                table[i] * (1.0 - frac) + table[i + 1] * frac
            }
            // ICC.1:2010 §10.16. Types 0–4; the higher ones fall back to their own
            // lower forms because the extra parameters are zero there by definition.
            Curve::Parametric { kind, g, a, b, c, d, e, f } => match kind {
                0 => x.powf(*g),
                1 => {
                    if x >= -b / a {
                        (a * x + b).powf(*g)
                    } else {
                        0.0
                    }
                }
                2 => {
                    if x >= -b / a {
                        (a * x + b).powf(*g) + c
                    } else {
                        *c
                    }
                }
                3 => {
                    if x >= *d {
                        (a * x + b).powf(*g)
                    } else {
                        c * x
                    }
                }
                _ => {
                    if x >= *d {
                        (a * x + b).powf(*g) + e
                    } else {
                        c * x + f
                    }
                }
            },
        }
    }
}

/// What a profile says, once it has been read.
#[derive(Clone, Debug, PartialEq)]
pub struct Icc {
    /// RGB → XYZ at D65, adapted from the profile's D50 colorants.
    pub to_xyz_d65: Mat3,
    /// The `wtpt` tag as stored, for reporting.
    pub white_point: [f64; 3],
    pub trc: [Curve; 3],
    pub bytes: usize,
}

impl Icc {
    /// Which of §4's two spaces this is, or why it is neither.
    pub fn classify(&self) -> Result<&'static Space, IccError> {
        let candidates = [&SRGB, &DISPLAY_P3];
        let (space, distance) = candidates
            .iter()
            .map(|s| (*s, matrix_distance(&self.to_xyz_d65, &s.to_xyz())))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .expect("two candidates");

        if distance > COLORANT_TOLERANCE {
            let m = &self.to_xyz_d65.0;
            return Err(IccError::UnknownSpace {
                red: [m[0][0], m[1][0], m[2][0]],
                green: [m[0][1], m[1][1], m[2][1]],
                blue: [m[0][2], m[1][2], m[2][2]],
                nearest: space.name,
                distance,
            });
        }

        // Primaries are half of a space. A profile with Display P3's primaries and a
        // 1.8 gamma is not Display P3, and the difference is visible rather than
        // subtle.
        let mut worst = 0.0f64;
        for i in 0..=16 {
            let x = i as f64 / 16.0;
            let want = space.transfer.to_linear(x as f32) as f64;
            for curve in &self.trc {
                worst = worst.max((curve.to_linear(x) - want).abs());
            }
        }
        if worst > CURVE_TOLERANCE {
            return Err(IccError::UnknownCurve { space: space.name, worst });
        }
        Ok(space)
    }
}

/// The largest component difference between two RGB→XYZ matrices.
fn matrix_distance(a: &Mat3, b: &Mat3) -> f64 {
    (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| (a.0[i][j] - b.0[i][j]).abs())
        .fold(0.0, f64::max)
}

/// Bradford chromatic adaptation between two white points, both as XYZ.
///
/// Needed here and nowhere else so far: `Space::linear_to` asserts that both ends are
/// D65 precisely so a future D50 space fails loudly instead of being silently wrong.
/// This is the one place a D50 value legitimately arrives, because the ICC connection
/// space is D50 by definition.
pub fn bradford(from: [f64; 3], to: [f64; 3]) -> Mat3 {
    let cone = |w: [f64; 3]| {
        let m = &BRADFORD.0;
        [
            m[0][0] * w[0] + m[0][1] * w[1] + m[0][2] * w[2],
            m[1][0] * w[0] + m[1][1] * w[1] + m[1][2] * w[2],
            m[2][0] * w[0] + m[2][1] * w[1] + m[2][2] * w[2],
        ]
    };
    let (s, d) = (cone(from), cone(to));
    let scale = Mat3([
        [d[0] / s[0], 0.0, 0.0],
        [0.0, d[1] / s[1], 0.0],
        [0.0, 0.0, d[2] / s[2]],
    ]);
    BRADFORD.invert().mul(&scale).mul(&BRADFORD)
}

/// Parse the parts of a profile §4 needs.
///
/// Every read is bounds-checked and every failure is an error. An ICC profile arrives
/// from a file somebody else wrote, and a panic in a decoder is a crash on opening a
/// photograph.
pub fn parse(bytes: &[u8]) -> Result<Icc, IccError> {
    if bytes.len() < 132 {
        return Err(IccError::NotAProfile(format!("{} bytes", bytes.len())));
    }
    if &bytes[36..40] != b"acsp" {
        return Err(IccError::NotAProfile(
            "no `acsp` signature at offset 36".into(),
        ));
    }

    let count = be_u32(bytes, 128)? as usize;
    // 12 bytes per tag entry. A count that does not fit is a malformed profile, not a
    // reason to read past the end of the buffer.
    if 132 + count.saturating_mul(12) > bytes.len() {
        return Err(IccError::NotAProfile(format!(
            "tag table claims {count} tags, which does not fit in {} bytes",
            bytes.len()
        )));
    }

    let mut tags = Vec::with_capacity(count);
    for i in 0..count {
        let at = 132 + i * 12;
        let sig = &bytes[at..at + 4];
        let offset = be_u32(bytes, at + 4)? as usize;
        let size = be_u32(bytes, at + 8)? as usize;
        let end = offset.checked_add(size).ok_or_else(|| {
            IccError::NotAProfile("a tag's offset and size overflow".into())
        })?;
        if end > bytes.len() {
            return Err(IccError::NotAProfile(format!(
                "tag `{}` runs past the end of the profile",
                String::from_utf8_lossy(sig)
            )));
        }
        tags.push((sig.to_vec(), &bytes[offset..end]));
    }
    let find = |want: &[u8]| tags.iter().find(|(sig, _)| sig == want).map(|(_, d)| *d);

    let colorant = |sig: &[u8; 4]| -> Result<[f64; 3], IccError> {
        let data = find(sig).ok_or_else(|| {
            IccError::Incomplete(format!("the `{}` tag", String::from_utf8_lossy(sig)))
        })?;
        read_xyz(data)
    };
    let (r, g, b) = (colorant(b"rXYZ")?, colorant(b"gXYZ")?, colorant(b"bXYZ")?);
    let white = find(b"wtpt").map(read_xyz).transpose()?.unwrap_or(PCS_D50);

    // Columns are the primaries, as stored: RGB → XYZ at the D50 connection space.
    let d50 = Mat3([
        [r[0], g[0], b[0]],
        [r[1], g[1], b[1]],
        [r[2], g[2], b[2]],
    ]);
    // …and the comparison that follows has to happen at D65, where our own spaces are
    // defined. This adaptation is the whole reason a naive colorant comparison fails.
    let d65 = super::colour::D65;
    let d65_xyz = [d65.x / d65.y, 1.0, (1.0 - d65.x - d65.y) / d65.y];
    let to_xyz_d65 = bradford(PCS_D50, d65_xyz).mul(&d50);

    let curve = |sig: &[u8; 4]| -> Result<Curve, IccError> {
        let data = find(sig).ok_or_else(|| {
            IccError::Incomplete(format!("the `{}` tag", String::from_utf8_lossy(sig)))
        })?;
        read_curve(data)
    };

    Ok(Icc {
        to_xyz_d65,
        white_point: white,
        trc: [curve(b"rTRC")?, curve(b"gTRC")?, curve(b"bTRC")?],
        bytes: bytes.len(),
    })
}

fn be_u32(bytes: &[u8], at: usize) -> Result<u32, IccError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| IccError::NotAProfile(format!("truncated at offset {at}")))
}

/// s15Fixed16Number: a signed 16.16 fixed-point value.
fn s15fixed16(bytes: &[u8], at: usize) -> Result<f64, IccError> {
    bytes
        .get(at..at + 4)
        .map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64 / 65536.0)
        .ok_or_else(|| IccError::NotAProfile(format!("truncated XYZ at offset {at}")))
}

/// `XYZType`: a four-byte signature, four reserved bytes, then three s15Fixed16.
fn read_xyz(data: &[u8]) -> Result<[f64; 3], IccError> {
    if data.len() < 20 || &data[0..4] != b"XYZ " {
        return Err(IccError::Incomplete("a well-formed XYZ tag".into()));
    }
    Ok([
        s15fixed16(data, 8)?,
        s15fixed16(data, 12)?,
        s15fixed16(data, 16)?,
    ])
}

fn read_curve(data: &[u8]) -> Result<Curve, IccError> {
    if data.len() < 12 {
        return Err(IccError::Incomplete("a well-formed tone curve".into()));
    }
    match &data[0..4] {
        b"curv" => {
            let n = be_u32(data, 8)? as usize;
            match n {
                0 => Ok(Curve::Identity),
                // A single entry is a gamma in u8Fixed8, not a one-point table.
                1 => {
                    let raw = data
                        .get(12..14)
                        .ok_or_else(|| IccError::Incomplete("the gamma value".into()))?;
                    Ok(Curve::Gamma(u16::from_be_bytes([raw[0], raw[1]]) as f64 / 256.0))
                }
                _ => {
                    let want = 12 + n * 2;
                    if data.len() < want {
                        return Err(IccError::Incomplete(format!(
                            "{n} curve entries; only {} bytes present",
                            data.len()
                        )));
                    }
                    Ok(Curve::Table(
                        (0..n)
                            .map(|i| {
                                let at = 12 + i * 2;
                                u16::from_be_bytes([data[at], data[at + 1]]) as f64 / 65535.0
                            })
                            .collect(),
                    ))
                }
            }
        }
        b"para" => {
            let kind = data
                .get(8..10)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .ok_or_else(|| IccError::Incomplete("a parametric curve type".into()))?;
            // ICC.1:2010 §10.16: types 0–4 take 1, 3, 4, 5 and 7 parameters.
            let expected = match kind {
                0 => 1,
                1 => 3,
                2 => 4,
                3 => 5,
                4 => 7,
                other => {
                    return Err(IccError::Incomplete(format!(
                        "a parametric curve of a type it knows; this is type {other}"
                    )));
                }
            };
            let mut p = [0.0f64; 7];
            for (i, slot) in p.iter_mut().enumerate().take(expected) {
                *slot = s15fixed16(data, 12 + i * 4)?;
            }
            Ok(Curve::Parametric {
                kind,
                g: p[0],
                a: p[1],
                b: p[2],
                c: p[3],
                d: p[4],
                e: p[5],
                f: p[6],
            })
        }
        other => Err(IccError::Incomplete(format!(
            "a tone curve it knows; this one is `{}`",
            String::from_utf8_lossy(other)
        ))),
    }
}
