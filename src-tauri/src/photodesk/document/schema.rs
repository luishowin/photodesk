//! The sidecar document, as §6.1 describes it.
//!
//! **This file is the schema's only home.** §0 freezes the document as "declarative,
//! versioned, human-readable" and §13 puts the editing model in the TypeScript front
//! end — but a schema written down twice is two schemas, and the one that drifts is
//! the one nobody is testing. The TypeScript types are *generated* from these
//! declarations (`document::bindings`), exactly as the WebGL2 preview loads
//! naga-generated GLSL rather than a hand-written twin: a twin would make "one
//! schema" untestable in the same way a twin shader made "one shader source"
//! untestable.
//!
//! ## Two rules from §6.1 that shape everything here
//!
//! **`params` is a fixed schema per `op` + `op_version`, and unknown keys are a
//! validation error rather than a silent no-op.** That is why every parameter struct
//! carries `deny_unknown_fields`, and why `params` is parsed in two steps rather than
//! by a serde tag — the discriminant is a *pair* of sibling fields, so nothing serde
//! offers dispatches on it. See [`super::params`].
//!
//! **Omitted keys mean identity, not zero**, which is why every parameter is an
//! `Option` even where zero happens to be the identity value. This is load-bearing
//! rather than tidy: §6.1 makes a tool preset something that *merges into* the stack,
//! so a preset that says nothing about exposure has to leave exposure alone. If
//! omission collapsed to a default at parse time, merging and overwriting would be
//! the same operation and the difference would be unrecoverable.

use serde::{Deserialize, Serialize};

use super::params::Params;

/// The schema version. Bumping this is a deliberate, migrated event (§6.2).
pub const SCHEMA_VERSION: u32 = 1;

/// The pipeline ordering this document's appearance was authored against (§5).
///
/// Separate from [`SCHEMA_VERSION`] on purpose: the schema describes what the file
/// *says*, and the pipeline version describes what the file *looks like*. A document
/// can be perfectly readable and still render differently than it did, and §6.3 gives
/// those two conditions different behaviour — reject versus warn.
pub const PIPELINE_VERSION: u32 = 1;

/// One sidecar, one image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Document {
    /// Schema version. Named `photodesk` in the file because that is what §6.1 shows,
    /// and because a bare `version` next to `pipeline_version` invites the reader to
    /// assume they move together.
    pub photodesk: u32,
    pub pipeline_version: u32,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Geometry::is_identity")]
    pub geometry: Geometry,
    pub stack: Vec<Layer>,
    pub output: Output,
}

/// What this document is *about*, and how to know it is still the same file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Source {
    /// Relative to the sidecar's own directory, never absolute.
    ///
    /// A photograph and its sidecar move together — into a backup, onto another
    /// machine, between drives — and an absolute path survives none of that. §0's
    /// "every cache is regenerable" has a sibling this makes explicit: nothing in the
    /// document may depend on where the library happens to sit today.
    pub file: String,
    /// `blake3:<hex>` of the source bytes.
    ///
    /// The prefix is not decoration. §12.3 hashes the source before and after a full
    /// open/edit/export cycle, and a bare hex string with no algorithm named is a
    /// value that cannot be re-verified once the algorithm changes.
    pub hash: String,
    /// `[width, height]` of the *source*, before orientation is applied.
    pub dimensions: [u32; 2],
    pub colorspace: ColorSpace,
    /// EXIF orientation, 1–8.
    pub orientation: u8,
}

/// The two spaces §4 handles. A third is a decision, not a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
}

/// §5 stage 1, which runs once for the whole image rather than per layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Geometry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
    /// Degrees, positive counter-clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f32>,
}

impl Geometry {
    /// True when this stage would do nothing, in which case §6.1 says it is skipped
    /// entirely at render time and — the part that matters here — is not written to
    /// the file at all. An empty `"geometry": {}` in a sidecar is noise that every
    /// future diff has to scroll past.
    pub fn is_identity(&self) -> bool {
        self.crop.is_none() && self.rotate.is_none()
    }
}

/// A crop in normalised source coordinates, so it survives a proxy change.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One entry in the stack. §5 makes the stack the loop and this the loop body.
///
/// `Deserialize` is hand-written rather than derived — see the impl below and
/// [`super::params`]. Everything else about the type is ordinary.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Layer {
    /// Stable for the life of the layer. §9.3's cache keys and §12.2's grain seed both
    /// derive from it, so renaming a layer must not change it and reordering the stack
    /// must not either.
    pub id: String,
    pub op: Op,
    pub op_version: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    /// `null` is a global adjustment — §5's loop body with an identity mask, not a
    /// separate execution model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Mask>,
    pub params: Params,
}

fn yes() -> bool {
    true
}

/// The one place §6.1's "`params` is a fixed schema per `op` + `op_version`" is
/// enforced, and the reason it cannot be a derive.
///
/// serde dispatches on a tag *inside* the object. Here the discriminant is two
/// sibling fields next to it, so the layer has to be read first and the params typed
/// second. `Repr` exists only to borrow the derive for the eight ordinary fields; it
/// is private, it is used once, and it is the smallest amount of duplication that
/// buys a `Document` which is valid by construction.
impl<'de> Deserialize<'de> for Layer {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            id: String,
            op: Op,
            op_version: u32,
            #[serde(default = "yes")]
            enabled: bool,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            opacity: Option<f32>,
            #[serde(default)]
            mask: Option<Mask>,
            params: serde_json::Value,
        }

        let r = Repr::deserialize(d)?;
        // An unknown `op` string never reaches here: `Op` is a closed enum, so serde
        // has already refused it by name above. That is §6.3's "unknown op → reject
        // the document", and it costs nothing because the set had to be closed anyway.
        let params = Params::from_value(r.op, r.op_version, r.params)
            .map_err(|e| serde::de::Error::custom(format!("layer `{}`: {e}", r.id)))?;
        Ok(Layer {
            id: r.id,
            op: r.op,
            op_version: r.op_version,
            enabled: r.enabled,
            name: r.name,
            opacity: r.opacity,
            mask: r.mask,
            params,
        })
    }
}

/// What a layer does. One member today; the enum exists because §6.3 has to be able
/// to *reject* an unknown one, which requires a closed set to compare against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum Op {
    Adjust,
}

/// §8's mask: components composed in declared order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Mask {
    pub op: MaskOp,
    pub components: Vec<MaskComponent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum MaskOp {
    Union,
    Intersect,
    Subtract,
}

/// §8's six component kinds.
///
/// Modelled in full at v0.1 although masks are a v0.4 feature, because the sidecar is
/// a *file format*: a v0.4 document that a v0.1 app cannot even parse is a different
/// and worse problem than one it can parse and declines to render. §6.3 already gives
/// the second case a defined behaviour and the first none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum MaskComponent {
    /// Vector stroke paths, rasterised on the GPU so proxy and export agree (§8).
    Brush {
        strokes: Vec<Stroke>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feather: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invert: Option<bool>,
    },
    Linear {
        from: [f32; 2],
        to: [f32; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feather: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invert: Option<bool>,
    },
    Radial {
        center: [f32; 2],
        radius: [f32; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feather: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invert: Option<bool>,
    },
    /// Reads **the layer's input**, per the frozen register item — not the source and
    /// not the final image (§8).
    Luminance {
        range: [f32; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feather: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invert: Option<bool>,
    },
    /// Likewise reads the layer's input.
    Color {
        target: [f32; 3],
        tolerance: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feather: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invert: Option<bool>,
    },
    Ai {
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feather: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invert: Option<bool>,
    },
}

/// A brush stroke in normalised source coordinates, so it is resolution-independent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub radius: f32,
    #[serde(default = "yes")]
    pub additive: bool,
}

/// §5 stage 13, as a saved intent rather than as something the exporter is told each
/// time. Round-tripping the export settings is what makes "export again, same result"
/// true across sessions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub struct Output {
    pub format: OutputFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    pub colorspace: ColorSpace,
    pub metadata: MetadataPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum OutputFormat {
    Jpeg,
    Png,
    Tiff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "document.ts"))]
pub enum MetadataPolicy {
    /// §6.1's default. GPS is dropped; everything else survives.
    KeepMinusGps,
    Keep,
    Strip,
}

impl Document {
    /// A document for a freshly opened image: no edits, everything at identity.
    ///
    /// The stack is **empty rather than seeded with a global layer**. §5's loop body
    /// skips identity stages entirely, so a seeded all-identity layer would be a row
    /// in the UI, a line in every diff and a node the graph has to eliminate, in
    /// exchange for nothing. The global layer is created when the first slider moves.
    pub fn new(source: Source) -> Self {
        Self {
            photodesk: SCHEMA_VERSION,
            pipeline_version: PIPELINE_VERSION,
            source,
            geometry: Geometry::default(),
            stack: Vec::new(),
            output: Output {
                format: OutputFormat::Jpeg,
                quality: Some(92),
                // §4: "Default export is sRGB, because that's what survives contact
                // with the internet."
                colorspace: ColorSpace::Srgb,
                metadata: MetadataPolicy::KeepMinusGps,
            },
        }
    }
}
