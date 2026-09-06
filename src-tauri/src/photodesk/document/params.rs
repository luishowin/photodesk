//! `params`, and the two-step parse §6.1's rule forces.
//!
//! > `params` is a **fixed schema per `op` + `op_version`**. Unknown keys are a
//! > validation error, never a silent no-op.
//!
//! Nothing serde offers dispatches on that, because the discriminant is a *pair of
//! sibling fields* rather than a tag inside the object. So [`Layer`] has a
//! hand-written `Deserialize` that reads `op` and `op_version` first and then hands
//! the raw `params` value to [`Params::from_value`]. The alternative — storing
//! `params` as untyped JSON and validating later — would mean a `Document` value in
//! memory could be invalid, and "a Document has been validated" is worth more than
//! the forty lines it costs.
//!
//! **What `adjust` version 1 covers.** Every scalar parameter of §5's stages 2–5 and
//! 9: white balance, exposure, the highlight/shadow/black trio, contrast, and
//! vibrance/saturation. That is nine fields where §14 gives v0.1 six sliders, and the
//! extra three are here deliberately — they belong to the same op, they are scalars
//! of the same shape, and adding them later would cost an `op_version` bump and a
//! migration to buy nothing. Stages 6–8 are *not* here: a tone curve, an HSL band set
//! and a grading wheel are structured rather than scalar, they arrive at v0.3, and
//! they will come with the version bump they actually justify.
//!
//! **Not validated: ranges.** A slider's travel is a UI decision (§11) and §6.1 states
//! no bounds, so inventing them here would put a number in the register's blind spot.
//! Finiteness *is* checked — a NaN that reaches the working space propagates through
//! every later stage and out into the exported file, and JSON cannot represent one, so
//! its only source is a writer that is already wrong.

use serde::{Deserialize, Serialize};

use super::schema::Op;

/// The parameters of one layer, selected by its `op` and `op_version`.
///
/// Serialised untagged: the discriminant already lives in the layer's own `op` and
/// `op_version` fields, and repeating it inside `params` would give a document two
/// places to disagree with itself.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum Params {
    /// `op: "adjust"`, `op_version: 1`.
    AdjustV1(AdjustV1),
}

/// §5 stages 2–5 and 9, as scalars.
///
/// Every field is `Option` because §6.1 says an omitted key means *identity, not
/// zero*. That distinction is load-bearing rather than tidy: a tool preset merges into
/// the stack, so a preset that says nothing about exposure has to leave exposure
/// alone — see [`AdjustV1::merge_from`], which is the operation the `Option` exists
/// for and which could not be written at all if omission collapsed to a default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct AdjustV1 {
    /// §5 stage 2. Kelvin, **as a delta rather than an absolute**.
    ///
    /// Forced by §5's execution model rather than chosen: the stack is the loop and
    /// stage 2 runs once per layer, so two layers each declaring an absolute 5200 K
    /// would describe nothing. The UI shows an absolute figure for the global layer
    /// because that is what a photographer reads; the file stores what composes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// §5 stage 2, green–magenta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<f32>,
    /// §5 stage 3, in stops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f32>,
    /// §5 stage 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlights: Option<f32>,
    /// §5 stage 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadows: Option<f32>,
    /// §5 stage 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blacks: Option<f32>,
    /// §5 stage 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f32>,
    /// §5 stage 9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibrance: Option<f32>,
    /// §5 stage 9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
}

impl AdjustV1 {
    /// Every field, so the two functions below cannot fall out of step with the struct
    /// when a tenth parameter is added.
    fn fields(&self) -> [Option<f32>; 9] {
        [
            self.temperature,
            self.tint,
            self.exposure,
            self.highlights,
            self.shadows,
            self.blacks,
            self.contrast,
            self.vibrance,
            self.saturation,
        ]
    }

    fn fields_mut(&mut self) -> [&mut Option<f32>; 9] {
        [
            &mut self.temperature,
            &mut self.tint,
            &mut self.exposure,
            &mut self.highlights,
            &mut self.shadows,
            &mut self.blacks,
            &mut self.contrast,
            &mut self.vibrance,
            &mut self.saturation,
        ]
    }

    /// True when this layer would do nothing. §6.1 skips identity stages at render
    /// time, and §6.2's graph eliminates identity nodes.
    pub fn is_identity(&self) -> bool {
        self.fields().iter().all(Option::is_none)
    }

    /// Overlay `other` onto `self`, which is what §6.1 means by a tool preset that
    /// *merges into* the stack rather than replacing it.
    ///
    /// A field the preset does not mention is left exactly as it was — including left
    /// absent. This is the whole reason the fields are `Option`, and it is why the
    /// distinction cannot be recovered after the fact: with a plain `f32`, "the preset
    /// sets exposure to 0" and "the preset says nothing about exposure" are the same
    /// value, and one of them silently discards the user's work.
    pub fn merge_from(&mut self, other: &AdjustV1) {
        for (dst, src) in self.fields_mut().into_iter().zip(other.fields()) {
            if src.is_some() {
                *dst = src;
            }
        }
    }

    /// The name of the first non-finite parameter, if any.
    pub(super) fn non_finite(&self) -> Option<&'static str> {
        const NAMES: [&str; 9] = [
            "temperature",
            "tint",
            "exposure",
            "highlights",
            "shadows",
            "blacks",
            "contrast",
            "vibrance",
            "saturation",
        ];
        self.fields()
            .iter()
            .zip(NAMES)
            .find(|(v, _)| v.is_some_and(|v| !v.is_finite()))
            .map(|(_, name)| name)
    }
}

/// Why a `params` object could not be typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamsError {
    /// §6.3: "Unknown `op` or `op_version` → reject the document."
    UnknownOpVersion { op: Op, op_version: u32 },
    /// §6.1: unknown keys are a validation error, and serde's message names the key.
    Invalid(String),
}

impl std::fmt::Display for ParamsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamsError::UnknownOpVersion { op, op_version } => write!(
                f,
                "no schema for op {op:?} at op_version {op_version}. §6.3 rejects the \
                 document rather than guessing: a partially-understood edit is worse \
                 than a refused one"
            ),
            ParamsError::Invalid(e) => write!(f, "params did not match the schema: {e}"),
        }
    }
}

impl std::error::Error for ParamsError {}

impl Params {
    /// Type a raw `params` object against the `(op, op_version)` pair that selects its
    /// schema. The single place §6.1's rule is enforced.
    pub fn from_value(
        op: Op,
        op_version: u32,
        value: serde_json::Value,
    ) -> Result<Self, ParamsError> {
        match (op, op_version) {
            (Op::Adjust, 1) => serde_json::from_value::<AdjustV1>(value)
                .map(Params::AdjustV1)
                .map_err(|e| ParamsError::Invalid(e.to_string())),
            (op, op_version) => Err(ParamsError::UnknownOpVersion { op, op_version }),
        }
    }

    /// The `(op, op_version)` this variant belongs to. Used to check that a layer's
    /// declared pair and its typed params still agree after the document has been
    /// edited in memory — they are two fields that must move together, so something
    /// has to say so.
    pub fn op(&self) -> (Op, u32) {
        match self {
            Params::AdjustV1(_) => (Op::Adjust, 1),
        }
    }

    pub fn is_identity(&self) -> bool {
        match self {
            Params::AdjustV1(p) => p.is_identity(),
        }
    }

    pub(super) fn non_finite(&self) -> Option<&'static str> {
        match self {
            Params::AdjustV1(p) => p.non_finite(),
        }
    }
}
