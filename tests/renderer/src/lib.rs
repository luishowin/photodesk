//! Spike C — preview renderer (`ARCHITECTURE.md` §2.3).
//!
//! §7.2's candidate is: author once in WGSL, run it natively through wgpu for export,
//! and transpile it to GLSL ES 3.0 with `naga` for the WebGL2 preview inside the
//! webview. Spike A appeared to kill that — but the compute chain with no GLSL target
//! was RapidRAW's, and there is no fork. Ours is authored fragment-first, which stays
//! inside what GLSL ES 3.0 has, so the branch is live again and this crate tests it
//! rather than assuming either outcome.

pub use photodesk::engine::glsl::{GlslError as TranspileError, Lowered as Transpiled, lower};

/// Lower one WGSL entry point to GLSL ES 3.00.
///
/// **This now delegates to `photodesk::engine::glsl`, and that is the point of the
/// move.** Spike C owned this function while it was answering a question; the answer
/// was yes, so §7.2's preview lowers the product's shaders at startup and the lowering
/// is product code. A copy kept here would mean the negative control below — that
/// compute is *refused* — was asserting it about a function nothing ships, which is the
/// shape of "green and meaningless" §2.2 already caught once in the colour harness.
pub fn to_glsl_es300(
    wgsl: &str,
    entry_point: &str,
    stage: naga::ShaderStage,
) -> Result<Transpiled, TranspileError> {
    lower(wgsl, entry_point, stage)
}

/// Spike C's own shader — deliberately **not** the product's.
///
/// §2.3 said the outcome "decides how every shader in the project is written", so this
/// one was built to be hostile: a large uniform block with fixed-size arrays in it,
/// dynamic indexing, a data-dependent loop bound and a switch. It is a stress test that
/// stays a stress test, and it exercises constructs the product's shaders do not have
/// yet — curves and HSL bands arrive at v0.3.
///
/// The product's fused pass is [`PRODUCT_ADJUST_WGSL`], and both are lowered here: this
/// one to keep the headroom finding honest, that one because it is what actually runs.
pub const ADJUST_WGSL: &str = include_str!("../shaders/adjust.wgsl");

/// §5 stages 2–9 as the product fuses them — `shaders/photodesk/adjust.wgsl`.
pub const PRODUCT_ADJUST_WGSL: &str = photodesk::engine::render::ADJUST_WGSL;

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
pub const ENCODE_WGSL: &str = photodesk::engine::render::ENCODE_WGSL;
