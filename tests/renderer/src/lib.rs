//! Spike C — preview renderer (`ARCHITECTURE.md` §2.3).
//!
//! §7.2's candidate is: author once in WGSL, run it natively through wgpu for export,
//! and transpile it to GLSL ES 3.0 with `naga` for the WebGL2 preview inside the
//! webview. Spike A appeared to kill that — but the compute chain with no GLSL target
//! was RapidRAW's, and there is no fork. Ours is authored fragment-first, which stays
//! inside what GLSL ES 3.0 has, so the branch is live again and this crate tests it
//! rather than assuming either outcome.

use naga::back::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};

/// What came out of a transpilation attempt.
#[derive(Debug)]
pub struct Transpiled {
    pub source: String,
    /// Bindings naga reassigned or dropped. WebGL2 has no descriptor sets, so
    /// `@group`/`@binding` has to collapse onto plain uniform locations, and knowing
    /// *how* is the difference between a working bind path and a silent mismatch.
    pub uniform_names: Vec<String>,
}

#[derive(Debug)]
pub enum TranspileError {
    Parse(String),
    Validate(String),
    Backend(String),
}

impl std::fmt::Display for TranspileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranspileError::Parse(e) => write!(f, "WGSL parse failed:\n{e}"),
            TranspileError::Validate(e) => write!(f, "WGSL validation failed:\n{e}"),
            TranspileError::Backend(e) => write!(f, "GLSL backend refused it:\n{e}"),
        }
    }
}

/// Lower one WGSL entry point to GLSL ES 3.00.
///
/// `version` is pinned to ES 3.00 deliberately: that is what WebGL2 exposes, and
/// letting naga pick a desktop profile would produce source the browser cannot
/// compile while reporting success here.
pub fn to_glsl_es300(
    wgsl: &str,
    entry_point: &str,
    stage: naga::ShaderStage,
) -> Result<Transpiled, TranspileError> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|e| TranspileError::Parse(e.emit_to_string(wgsl)))?;

    // Validate with the capabilities WebGL2 actually has — not the default set, which
    // would wave through constructs the target cannot express.
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|e| TranspileError::Validate(format!("{e:?}")))?;

    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..Default::default()
    };
    let pipeline_options = glsl::PipelineOptions {
        shader_stage: stage,
        entry_point: entry_point.to_string(),
        multiview: None,
    };

    let mut source = String::new();
    let mut writer = glsl::Writer::new(
        &mut source,
        &module,
        &info,
        &options,
        &pipeline_options,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|e| TranspileError::Backend(format!("{e:?}")))?;

    let reflection = writer
        .write()
        .map_err(|e| TranspileError::Backend(format!("{e:?}")))?;

    let mut uniform_names: Vec<String> = reflection
        .uniforms
        .values()
        .cloned()
        .chain(reflection.texture_mapping.keys().cloned())
        .collect();
    uniform_names.sort();

    Ok(Transpiled {
        source,
        uniform_names,
    })
}

/// The shader under test: §5 stages 2–9 fused into one pass.
pub const ADJUST_WGSL: &str = include_str!("../shaders/adjust.wgsl");

/// §5 stage 13 — the output encode, with §16 #11's gamut policy in it.
///
/// **The product's shader, not the spike's.** It lives in `shaders/photodesk/`, which
/// §13 names as the one shader source, and this crate reads it from there — so the
/// lowering and agreement tests below are about the shader that ships rather than
/// about a copy that happens to look like it.
///
/// A separate source from [`ADJUST_WGSL`] because it is a separate pass, not a
/// separate *path*: stage 13 runs once at the end of the chain while stages 2–12 run
/// once per layer (§5), and §0's one-shader-source invariant is about preview and
/// export sharing a source, not about the whole pipeline being one file.
pub const ENCODE_WGSL: &str = include_str!("../../../shaders/photodesk/encode.wgsl");
