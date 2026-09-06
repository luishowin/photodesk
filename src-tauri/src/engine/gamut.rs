//! Export gamut mapping — the policy §4 never named (§16 #11).
//!
//! §4 says "linear P3 → tone encode → sRGB (default) or Display P3 → ICC-tagged file"
//! and stops. A Display P3 photograph exported to sRGB produces channels outside
//! [0,1] for everything the smaller gamut cannot hold, and *something* decides what
//! happens to them. Until now that something was one `clamp` in `working.rs`.
//!
//! ## Why this cannot be delegated
//!
//! The obvious move is to let lcms2 decide — ask for a perceptual intent and ship
//! whatever comes back. That does not work here and the reason is structural rather
//! than a matter of taste: perceptual rendering lives in a profile's B2A lookup
//! tables, and neither sRGB nor Display P3 has any. They are matrix/TRC profiles, so
//! every intent collapses to the same colorimetric transform plus a clip.
//! `gamut_policy.rs` asserts that rather than repeating it — if a future lcms2 starts
//! distinguishing them, the test says so.
//!
//! So the policy is ours to write, and it is ours to write **in a fragment shader**:
//! stage 13 is the output encode, §0 freezes one shader source across preview and
//! export, and the preview runs it per pixel per frame. Every policy in
//! [`GamutPolicy`] is therefore a closed form of a few dozen ALU ops with no loop and
//! no table. The one candidate that is not — nearest-in-Lab, which needs a search —
//! is deliberately *not* a variant of that enum. It lives below as
//! [`nearest_in_gamut_lab`], a control in the same spirit as the compute-shader
//! negative control in `tests/renderer/`: it exists to show what the ΔE-minimising
//! answer looks like, and to make it obvious that the ΔE-minimising answer is not the
//! one to ship.
//!
//! ## The geometry
//!
//! In linear destination RGB the gamut is exactly the unit cube. Every policy here
//! except the clip works along the ray from the achromatic colour of the *same
//! luminance* out to the sample, because that ray has one useful property: the vector
//! along it carries zero luminance, so scaling it changes chroma and leaves Y — and
//! therefore L\* — untouched. "Preserve lightness exactly, spend chroma" is a
//! statement about the maths rather than a hope about the result.
//!
//! Written once, generically over [`Real`], and instantiated at f32 and f64 for the
//! same reason the workload is: two copies of a policy are two policies.

use super::colour::{Real, Space};

/// **The policy §4 exports under — §16 #11, frozen 2026-09-06.** One named constant,
/// so the decision has a home rather than living as a `clamp` inside `emit`.
///
/// Chosen over the clip on `gamut_policy.rs`'s measurements, and the reasons fit in
/// three sentences. The clip's error has no policy: how much lightness a colour loses
/// depends on which channel happened to saturate first, measured at up to 2.8 L\*.
/// This one's error is stated — lightness is exact, chroma is what gets spent — and on
/// a real photograph it halves the number of adjacent pixel pairs that merge into one
/// colour, for zero cost inside the gamut and the same handful of ALU ops.
/// [`GamutPolicy::CompressLuma`] halves it again, and is not chosen because it buys
/// that by moving 3% of the frame that was already correct, on the strength of a knee
/// with no derivation behind it.
pub const EXPORT_GAMUT_POLICY: GamutPolicy = GamutPolicy::PreserveLuma;

/// A ray length long enough to stand in for "unconstrained".
///
/// Not `f32::INFINITY`. This function has to be readable as the shader it becomes,
/// and GLSL ES 3.00 has no infinity literal — so the sentinel is a number. At 1e9 the
/// reciprocal is 1e-9, which every branch below already treats as comfortably inside
/// the gamut, so the sentinel decays into the identity rather than into a special case.
const UNCONSTRAINED: f64 = 1.0e9;

/// Below this the channel is not moving along the ray and imposes no bound. Guards a
/// division, nothing more.
const FLAT: f64 = 1.0e-9;

/// What happens to a colour the destination gamut cannot hold.
///
/// Every variant is a closed form. That is a hard requirement, not an aesthetic one:
/// stage 13 runs in the preview's fragment shader as well as in the exporter, and §0
/// freezes the two to one source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GamutPolicy {
    /// Leave out-of-range values alone. Only meaningful for measurement — an encoder
    /// has to do *something* with a negative channel, and this is not it.
    None,

    /// Clamp each channel to [0,1] after the matrix, in linear light.
    ///
    /// Worth naming precisely, because the name flatters it: clamping to a box **is**
    /// the Euclidean projection onto that box, so this returns the nearest in-gamut
    /// colour measured in linear RGB. It minimises an error — just not one anybody
    /// perceives. Lightness and hue are both free to move, and out-of-gamut
    /// neighbours collapse onto the same face.
    ClipLinear,

    /// Slide along the constant-luminance ray until the colour is exactly on the
    /// gamut boundary, and no further.
    ///
    /// Identity inside the gamut, ΔL\* = 0 by construction, and hue held in linear
    /// RGB. Same cost as the clip, same in-gamut exactness, strictly better
    /// colorimetry — but it still puts everything outside the gamut *on* the
    /// boundary, so a gradient that runs off the edge still arrives flat.
    PreserveLuma,

    /// The same ray, with a smooth rational rolloff so that out-of-gamut colours land
    /// *inside* the boundary and stay distinguishable from each other.
    ///
    /// `knee` is where the rolloff starts, as a fraction of the distance to the
    /// boundary: below it nothing moves at all, above it the mapping is monotone and
    /// asymptotes to the boundary. It buys gradient survival, and it pays for it by
    /// moving in-gamut colours that were already correct — which is the whole trade,
    /// and the reason the knee is measured rather than guessed.
    CompressLuma { knee: f32 },
}

impl GamutPolicy {
    /// The luminance-preserving policies need the destination's Y weights; the others
    /// ignore them. Split out so a caller with a `Space` in hand can use [`map`].
    ///
    /// [`map`]: GamutPolicy::map
    pub fn map(self, c: [f32; 3], dst: &Space) -> [f32; 3] {
        self.map_with(c, luma_weights(dst))
    }

    /// Map a linear colour, already in the destination space, into the unit cube.
    ///
    /// Generic over precision so the harness's f64 reference chain and its f32
    /// pipeline run the *same* policy rather than two transcriptions of it.
    pub fn map_with<T: Real>(self, c: [T; 3], weights: [f64; 3]) -> [T; 3] {
        clamp_cube(self.map_unclamped(c, weights))
    }

    /// The policy without the final safety clamp.
    ///
    /// Exists so a test can ask how much work that clamp is doing. For the ray
    /// policies the answer has to be "none, to within float error" — they solve for
    /// the boundary, so a clamp that moves anything means the solution is wrong and
    /// the clamp is covering for it. For [`GamutPolicy::ClipLinear`] the clamp *is*
    /// the policy, and this returns the input unchanged.
    pub fn map_unclamped<T: Real>(self, c: [T; 3], weights: [f64; 3]) -> [T; 3] {
        let one = T::from_f64(1.0);

        match self {
            GamutPolicy::None => c,

            // The clamp in `map_with` is the whole of this policy.
            GamutPolicy::ClipLinear => c,

            GamutPolicy::PreserveLuma => {
                let r = Ray::of(c, weights);
                // The sample sits at t = 1, so `t_max >= 1` *is* the in-gamut test.
                // Returned unmodified rather than recomputed as `grey + 1.0 * v`,
                // which is the same colour only to within an ulp — and "in-gamut
                // content does not move" is a claim this file makes exactly.
                if r.t_max.to_f64() >= 1.0 {
                    return c;
                }
                r.at(r.t_max)
            }

            GamutPolicy::CompressLuma { knee } => {
                let r = Ray::of(c, weights);
                // Distance to the sample in units of the boundary: 1.0 sits on it.
                // `t_max` is floored rather than assumed positive because a colour at
                // the destination's white luminance has a zero-length ray — the only
                // in-gamut colour of that luminance is white itself. A real case at
                // the top of every blown highlight, not a degenerate one.
                let d = one.div(r.t_max.max(T::from_f64(FLAT)));
                if d.to_f64() <= knee as f64 {
                    return c;
                }
                // Rational rolloff: value and slope continuous at the knee, monotone
                // in `d`, asymptotic to the boundary. Chosen over an exponential
                // because it is one divide in a shader rather than a transcendental.
                let k = T::from_f64(knee as f64);
                let span = one.sub(k);
                let u = d.sub(k);
                let mapped = k.add(u.mul(span).div(span.add(u)));
                r.at(r.t_max.mul(mapped))
            }
        }
    }

    /// A label short enough for a table column.
    pub fn label(self) -> String {
        match self {
            GamutPolicy::None => "none".into(),
            GamutPolicy::ClipLinear => "clip-linear".into(),
            GamutPolicy::PreserveLuma => "preserve-luma".into(),
            GamutPolicy::CompressLuma { knee } => format!("compress k={knee:.2}"),
        }
    }
}

/// The ray from the achromatic colour of the sample's own luminance out to the sample.
struct Ray<T> {
    /// The achromatic point, as a single channel value — `(grey, grey, grey)` has
    /// luminance `grey` because the weights sum to one.
    grey: T,
    /// Sample minus achromatic point. Carries **zero luminance**, which is what makes
    /// scaling it a pure chroma operation.
    v: [T; 3],
    /// The largest multiple of `v` that stays inside the cube.
    t_max: T,
}

impl<T: Real> Ray<T> {
    fn of(c: [T; 3], w: [f64; 3]) -> Self {
        let zero = T::from_f64(0.0);
        let one = T::from_f64(1.0);

        // Clamped, because a highlight brighter than the destination's white has no
        // achromatic point of its own luminance to aim at.
        let grey = c[0]
            .mul(T::from_f64(w[0]))
            .add(c[1].mul(T::from_f64(w[1])))
            .add(c[2].mul(T::from_f64(w[2])))
            .max(zero)
            .min(one);

        let v = [c[0].sub(grey), c[1].sub(grey), c[2].sub(grey)];

        let mut t_max = T::from_f64(UNCONSTRAINED);
        for vi in v.iter() {
            let s = vi.to_f64();
            if s > FLAT {
                t_max = t_max.min(one.sub(grey).div(*vi));
            } else if s < -FLAT {
                t_max = t_max.min(zero.sub(grey).div(*vi));
            }
        }

        Self {
            grey,
            v,
            t_max: t_max.max(zero),
        }
    }

    fn at(&self, t: T) -> [T; 3] {
        [
            self.grey.add(self.v[0].mul(t)),
            self.grey.add(self.v[1].mul(t)),
            self.grey.add(self.v[2].mul(t)),
        ]
    }
}

/// A last clamp, for float error at the boundary rather than for gamut mapping.
///
/// The ray arithmetic lands on the face to within an ulp or two; the encoder still
/// needs a value it can quantise. Anything this clamp moves by more than ~1e-6 is a
/// bug in the policy above, and `gamut_policy.rs` asserts exactly that.
fn clamp_cube<T: Real>(c: [T; 3]) -> [T; 3] {
    let zero = T::from_f64(0.0);
    let one = T::from_f64(1.0);
    [
        c[0].max(zero).min(one),
        c[1].max(zero).min(one),
        c[2].max(zero).min(one),
    ]
}

/// Luminance weights for a space: the Y row of its RGB→XYZ matrix.
///
/// Derived from chromaticities like everything else in `colour.rs` — the familiar
/// (0.2126, 0.7152, 0.0722) is sRGB's, and writing it down would silently apply
/// sRGB's weights to a Display P3 export. They sum to one by construction, which is
/// what makes `(grey, grey, grey)` have luminance `grey`.
pub fn luma_weights(space: &Space) -> [f64; 3] {
    space.to_xyz().0[1]
}
