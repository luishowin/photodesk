//! A representative per-pixel workload, so the pass count measures something.
//!
//! The first version of this harness ran `passes` *identity* stages and reported a
//! flat line at 1, 5 and 30 passes. That result was arithmetically correct and
//! completely uninformative: `f16 -> f32 -> f16` is idempotent, so an identity pass
//! is a no-op and thirty of them are thirty no-ops. It would have supported the
//! sentence "f16 survives a thirty-pass chain" on no evidence at all.
//!
//! What actually costs precision is arithmetic between stores, which moves a value off
//! the f16 grid so the next store has to re-round it. So the workload below does real
//! per-pixel maths of the shape §5 stages 2–9 have — a gain, a tonal S-curve, a channel
//! mix — and the cost of f16 is measured as divergence from the identical workload
//! carried out entirely in f64.
//!
//! The workload is written **once**, generically, and instantiated at both precisions.
//! Two copies would be free to drift, and a drifting reference measures nothing.

/// The minimum float surface the workload needs. Implemented for `f32` and `f64` so
/// the pipeline and its reference cannot diverge by editing one and not the other.
pub trait Real: Copy {
    fn from_f64(v: f64) -> Self;
    fn to_f64(self) -> f64;
    fn add(self, o: Self) -> Self;
    fn sub(self, o: Self) -> Self;
    fn mul(self, o: Self) -> Self;
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

/// What one render pass does to a pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// Nothing. Kept only to demonstrate why it is the wrong instrument: with an
    /// identity workload the pass sweep is flat by construction.
    Identity,
    /// A gain, a tonal S-curve and a small channel mix — the shape of §5 stages 2–9,
    /// at magnitudes small enough that thirty passes stay inside a plausible edit
    /// rather than crushing the image into a corner where nothing is measurable.
    Representative,
}

impl Workload {
    /// Apply pass `i` of this workload. Generic over precision by construction.
    pub fn apply<T: Real>(self, v: [T; 3], i: u32) -> [T; 3] {
        match self {
            Workload::Identity => v,
            Workload::Representative => {
                // Alternating gain, so a long chain wanders around the starting
                // exposure instead of marching off to black or to clipping.
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                let gain = T::from_f64((sign * 0.05f64).exp2());
                let g = [v[0].mul(gain), v[1].mul(gain), v[2].mul(gain)];

                // A gentle smoothstep contrast, blended lightly. Clamped input because
                // the curve is only meaningful on [0,1]; out-of-gamut channels pass
                // through the blend unchanged, which is also what a real stage does.
                let zero = T::from_f64(0.0);
                let one = T::from_f64(1.0);
                let three = T::from_f64(3.0);
                let two = T::from_f64(2.0);
                let amount = T::from_f64(0.08);
                let mut c = [zero; 3];
                for (k, cell) in c.iter_mut().enumerate() {
                    let x = g[k].max(zero).min(one);
                    let s = x.mul(x).mul(three.sub(two.mul(x)));
                    *cell = g[k].add(s.sub(x).mul(amount));
                }

                // A small channel mix, the shape a white-balance or calibration matrix
                // has. Rows sum to one so a neutral stays neutral.
                let m = T::from_f64(0.02);
                let keep = T::from_f64(0.96);
                [
                    c[0].mul(keep).add(c[1].mul(m)).add(c[2].mul(m)),
                    c[1].mul(keep).add(c[0].mul(m)).add(c[2].mul(m)),
                    c[2].mul(keep).add(c[0].mul(m)).add(c[1].mul(m)),
                ]
            }
        }
    }
}
