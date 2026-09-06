//! Colour primitives: primaries, transfer functions, and the matrices between them.
//!
//! Everything here is derived from chromaticities at construction time rather than
//! written down as a table of pre-computed constants. Constants copied from a website
//! are the classic source of a silent half-percent error, and §2.2 exists to catch
//! exactly that class of thing — so the harness should not contain any.

/// A CIE 1931 xy chromaticity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

impl Xy {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// XYZ for this chromaticity normalised to Y = 1.
    fn to_xyz(self) -> [f64; 3] {
        [self.x / self.y, 1.0, (1.0 - self.x - self.y) / self.y]
    }
}

/// D65, as specified for sRGB and Display P3.
pub const D65: Xy = Xy::new(0.3127, 0.3290);

/// An RGB colour space: three primaries, a white point, and a transfer function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Space {
    pub name: &'static str,
    pub red: Xy,
    pub green: Xy,
    pub blue: Xy,
    pub white: Xy,
    pub transfer: Transfer,
}

/// sRGB / IEC 61966-2-1. Rec.709 primaries, D65, the piecewise sRGB curve.
pub const SRGB: Space = Space {
    name: "sRGB",
    red: Xy::new(0.640, 0.330),
    green: Xy::new(0.300, 0.600),
    blue: Xy::new(0.150, 0.060),
    white: D65,
    transfer: Transfer::Srgb,
};

/// Display P3. DCI-P3 primaries, D65 white, and the *sRGB* transfer curve —
/// not DCI's pure 2.6 gamma. Getting that wrong is a ~4 ΔE error that looks
/// like a gamut problem and is not one.
pub const DISPLAY_P3: Space = Space {
    name: "Display P3",
    red: Xy::new(0.680, 0.320),
    green: Xy::new(0.265, 0.690),
    blue: Xy::new(0.150, 0.060),
    white: D65,
    transfer: Transfer::Srgb,
};

/// The candidate working space of §4: Display P3 primaries, no encoding curve.
pub const LINEAR_P3: Space = Space {
    name: "linear Display P3",
    red: DISPLAY_P3.red,
    green: DISPLAY_P3.green,
    blue: DISPLAY_P3.blue,
    white: D65,
    transfer: Transfer::Linear,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transfer {
    /// The piecewise sRGB curve. Used by both sRGB and Display P3.
    Srgb,
    /// Identity. Scene values are already linear.
    Linear,
}

impl Transfer {
    /// Encoded [0,1] -> linear. Defined for values outside [0,1] by odd extension,
    /// so out-of-gamut channels survive a round trip instead of being silently clipped
    /// at a point where nothing has decided they should be.
    pub fn to_linear(self, v: f32) -> f32 {
        match self {
            Transfer::Linear => v,
            Transfer::Srgb => {
                let s = v.signum();
                let a = v.abs();
                s * if a <= 0.040_449_936 {
                    a / 12.92
                } else {
                    ((a + 0.055) / 1.055).powf(2.4)
                }
            }
        }
    }

    /// Linear -> encoded [0,1], the inverse of [`Transfer::to_linear`].
    pub fn from_linear(self, v: f32) -> f32 {
        match self {
            Transfer::Linear => v,
            Transfer::Srgb => {
                let s = v.signum();
                let a = v.abs();
                s * if a <= 0.003_130_8 {
                    a * 12.92
                } else {
                    1.055 * a.powf(1.0 / 2.4) - 0.055
                }
            }
        }
    }
}

/// A row-major 3x3 matrix. Built in f64, applied in f32.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3(pub [[f64; 3]; 3]);

impl Mat3 {
    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [
            (m[0][0] * v[0] as f64 + m[0][1] * v[1] as f64 + m[0][2] * v[2] as f64) as f32,
            (m[1][0] * v[0] as f64 + m[1][1] * v[1] as f64 + m[1][2] * v[2] as f64) as f32,
            (m[2][0] * v[0] as f64 + m[2][1] * v[1] as f64 + m[2][2] * v[2] as f64) as f32,
        ]
    }

    pub fn mul(&self, other: &Mat3) -> Mat3 {
        let mut out = [[0.0f64; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.0[i][k] * other.0[k][j]).sum();
            }
        }
        Mat3(out)
    }

    pub fn invert(&self) -> Mat3 {
        let m = &self.0;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        assert!(det.abs() > 1e-12, "singular matrix");
        let inv_det = 1.0 / det;
        Mat3([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
            ],
        ])
    }
}

impl Space {
    /// Linear RGB in this space -> CIE XYZ, derived from the chromaticities.
    pub fn to_xyz(&self) -> Mat3 {
        let r = self.red.to_xyz();
        let g = self.green.to_xyz();
        let b = self.blue.to_xyz();

        // Columns are the primaries.
        let primaries = Mat3([
            [r[0], g[0], b[0]],
            [r[1], g[1], b[1]],
            [r[2], g[2], b[2]],
        ]);

        // Scale each primary so the space's white RGB (1,1,1) maps to its white point.
        let w = self.white.to_xyz();
        let inv = primaries.invert();
        let s = [
            inv.0[0][0] * w[0] + inv.0[0][1] * w[1] + inv.0[0][2] * w[2],
            inv.0[1][0] * w[0] + inv.0[1][1] * w[1] + inv.0[1][2] * w[2],
            inv.0[2][0] * w[0] + inv.0[2][1] * w[1] + inv.0[2][2] * w[2],
        ];

        let mut m = primaries.0;
        for row in m.iter_mut() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell *= s[j];
            }
        }
        Mat3(m)
    }

    pub fn from_xyz(&self) -> Mat3 {
        self.to_xyz().invert()
    }

    /// Linear RGB in this space -> linear RGB in `dst`.
    ///
    /// Both spaces this project uses are D65, so no chromatic adaptation is applied.
    /// The assertion makes that assumption load-bearing rather than silent: a future
    /// D50 space (ICC's PCS white, or ProPhoto) will fail here rather than be wrong.
    pub fn linear_to(&self, dst: &Space) -> Mat3 {
        assert!(
            (self.white.x - dst.white.x).abs() < 1e-9 && (self.white.y - dst.white.y).abs() < 1e-9,
            "chromatic adaptation is not implemented: {} is {:?}, {} is {:?}",
            self.name,
            self.white,
            dst.name,
            dst.white
        );
        dst.from_xyz().mul(&self.to_xyz())
    }
}

/// The minimum float surface a per-pixel operation needs, implemented for `f32` and
/// `f64`.
///
/// Here rather than in the harness because the gamut policy is generic over it, and
/// the policy ships. Its purpose is the harness's own rule — write the maths **once**
/// and instantiate it at both precisions, because two copies are free to drift and a
/// drifting reference measures nothing.
pub trait Real: Copy {
    fn from_f64(v: f64) -> Self;
    fn to_f64(self) -> f64;
    fn add(self, o: Self) -> Self;
    fn sub(self, o: Self) -> Self;
    fn mul(self, o: Self) -> Self;
    fn div(self, o: Self) -> Self;
    fn max(self, o: Self) -> Self;
    fn min(self, o: Self) -> Self;
}

macro_rules! impl_real {
    ($t:ty) => {
        impl Real for $t {
            #[inline]
            fn from_f64(v: f64) -> Self {
                v as $t
            }
            #[inline]
            fn to_f64(self) -> f64 {
                self as f64
            }
            #[inline]
            fn add(self, o: Self) -> Self {
                self + o
            }
            #[inline]
            fn sub(self, o: Self) -> Self {
                self - o
            }
            #[inline]
            fn mul(self, o: Self) -> Self {
                self * o
            }
            #[inline]
            fn div(self, o: Self) -> Self {
                self / o
            }
            #[inline]
            fn max(self, o: Self) -> Self {
                <$t>::max(self, o)
            }
            #[inline]
            fn min(self, o: Self) -> Self {
                <$t>::min(self, o)
            }
        }
    };
}

impl_real!(f32);
impl_real!(f64);
