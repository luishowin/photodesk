//! WGSL → GLSL ES 3.00, which is the preview half of §7.2.
//!
//! §0 freezes one shader source for preview and export. Export runs `shaders/photodesk/`
//! natively through wgpu (`render.rs`); the preview runs the same files inside the
//! webview, and this is the step between. Spike C proved the path (`SPIKE-C.md`): the
//! chain lowers, WebKitGTK compiles it, and the two renderers agree to max 0.0088 over
//! 196,608 samples.
//!
//! **It runs at startup rather than at build time, and that is the point.** A generated
//! `.frag` on disk is a second artefact that can be stale — one edit to the WGSL and a
//! forgotten build step is a preview rendering last week's shader while the export
//! renders this week's, which is precisely the WYSIWYG drift §0 exists to prevent. The
//! lowering is cheap (a few milliseconds, once) and there is nothing to keep in sync.
//!
//! This module was Spike C's; `tests/renderer/` now calls it rather than keeping its
//! own, so the negative control asserting that compute is *refused* is asserting it
//! about the code that ships.

use naga::back::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};

/// What came out of a lowering.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub source: String,
    /// Bindings naga reassigned or dropped. WebGL2 has no descriptor sets, so
    /// `@group`/`@binding` collapses onto plain uniform locations, and knowing *how* is
    /// the difference between a working bind path and a silent mismatch.
    pub uniform_names: Vec<String>,
}

#[derive(Debug)]
pub enum GlslError {
    Parse(String),
    Validate(String),
    /// The construct has no GLSL ES 3.00 equivalent — compute, a storage buffer, a
    /// storage texture. A frozen register item exists so this never fires in the
    /// product; `tests/renderer/` keeps a negative control that it still fires when it
    /// should.
    Backend(String),
}

impl std::fmt::Display for GlslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GlslError::Parse(e) => write!(f, "WGSL parse failed:\n{e}"),
            GlslError::Validate(e) => write!(f, "WGSL validation failed:\n{e}"),
            GlslError::Backend(e) => write!(f, "the GLSL backend refused it:\n{e}"),
        }
    }
}

impl std::error::Error for GlslError {}

/// Lower one WGSL entry point to GLSL ES 3.00.
///
/// `version` is pinned to ES 3.00 deliberately: that is what WebGL2 exposes, and letting
/// naga pick a desktop profile would produce source the browser cannot compile while
/// reporting success here.
pub fn lower(
    wgsl: &str,
    entry_point: &str,
    stage: naga::ShaderStage,
) -> Result<Lowered, GlslError> {
    let module =
        naga::front::wgsl::parse_str(wgsl).map_err(|e| GlslError::Parse(e.emit_to_string(wgsl)))?;

    // Validate with the capabilities WebGL2 actually has — not the default set, which
    // would wave through constructs the target cannot express.
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|e| GlslError::Validate(format!("{e:?}")))?;

    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        // Stated rather than inherited from `Default`, because the front end's blit
        // depends on it. `ADJUST_COORDINATE_SPACE` negates `gl_Position.y`, which is
        // what makes a pass in GL write the same row indices as the same pass in wgpu
        // — the reason §12.2 agrees to the code instead of agreeing upside down, and
        // the reason `preview.ts` flips exactly once, at the end, rather than tracking
        // a parity. It is naga's default today; a release that changed the default
        // would silently turn every previewed photograph upside down, and this line is
        // what stops that from being a question about naga's changelog.
        writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
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
    .map_err(|e| GlslError::Backend(format!("{e:?}")))?;

    let reflection = writer
        .write()
        .map_err(|e| GlslError::Backend(format!("{e:?}")))?;

    let mut uniform_names: Vec<String> = reflection
        .uniforms
        .values()
        .cloned()
        .chain(reflection.texture_mapping.keys().cloned())
        .collect();
    uniform_names.sort();

    Ok(Lowered {
        source,
        uniform_names,
    })
}

/// Both stages of one shader, which is what a WebGL2 program needs.
pub fn lower_program(wgsl: &str) -> Result<(Lowered, Lowered), GlslError> {
    Ok((
        lower(wgsl, VERTEX_ENTRY, naga::ShaderStage::Vertex)?,
        lower(wgsl, FRAGMENT_ENTRY, naga::ShaderStage::Fragment)?,
    ))
}

/// The entry points every shader in `shaders/photodesk/` uses.
pub const VERTEX_ENTRY: &str = "vs_main";
pub const FRAGMENT_ENTRY: &str = "fs_main";
