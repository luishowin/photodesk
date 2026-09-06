//! PhotoDesk's window.
//!
//! Everything this file does is carry things across the webview boundary. The document
//! model, the graph compile, decode, render and export all live in `photodesk`, which
//! knows nothing about a window and is tested without one (§12.3, §12.1) — see this
//! crate's `Cargo.toml` for why that separation is a crate rather than a feature flag.
//!
//! **What crosses, and what deliberately does not.** §7.2 puts the preview inside the
//! webview precisely so that pixels do *not* cross per frame; RapidRAW's Linux path
//! ships a mozjpeg-encoded frame over IPC and that is the far end of the trade. So the
//! proxy crosses **once**, on open, as raw bytes rather than JSON. After that a slider
//! drag touches no IPC at all: the front end writes nine floats into a uniform buffer
//! and draws.
//!
//! Three things cross that could have been re-implemented in TypeScript, and each is
//! here because re-implementing it would have made a second source of something §0
//! freezes at one:
//!
//! - **the shaders**, lowered from `shaders/photodesk/` at startup (`engine::glsl`);
//! - **the compiled graph**, because §0 says it is compiled once, in Rust;
//! - **stage 13's uniform**, because its matrix, luma weights and gamut constants are
//!   derived rather than typed, and a TypeScript copy would drift silently.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use half::f16;
use photodesk::engine::colour::{DISPLAY_P3, SRGB, Space};
use photodesk::engine::image::Image;
use photodesk::engine::render::Renderer;
use photodesk::engine::{decode, export, glsl, render};
use photodesk::photodesk::document::{ColorSpace, Document, Source};
use photodesk::photodesk::graph::{self, Graph};
use photodesk::photodesk::sidecar;
use serde::Serialize;
use tauri::Manager;
use tauri::ipc::Response;

// ------------------------------------------------------------------- the session

/// One open photograph. There is exactly one, because §14's v0.1 is one screen.
struct Session {
    path: PathBuf,
    /// Full resolution, linear P3 — what the export renders from.
    full: Image,
    /// §7.1's proxy — what the preview renders from.
    proxy: Image,
    /// The source bytes, kept for §6.1's metadata policy on export. A 12 MP HEIC is a
    /// few megabytes; re-reading the file at export time would risk reading a *different*
    /// file, which §12.3 exists to make impossible.
    bytes: Vec<u8>,
}

#[derive(Default)]
struct State {
    session: Mutex<Option<Session>>,
    /// Built once. Adapter enumeration on the export path would be a visible stall, and
    /// §7.3 budgets exports as "never blocks the UI".
    renderer: Mutex<Option<Renderer>>,
}

/// Anything that goes wrong, as a string the front end can show.
///
/// A string rather than a typed enum on purpose: every one of these is already a typed
/// error on the Rust side with a written message (`DecodeError` names the missing codec
/// and its package, `CompileError` names the pipeline mismatch), and re-encoding that
/// taxonomy in TypeScript would be a second description of the same failures. The front
/// end shows the sentence; the sentence is the contract.
type Fallible<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ------------------------------------------------------------------- the shaders

#[derive(Serialize)]
struct Shaders {
    adjust_vert: String,
    adjust_frag: String,
    encode_vert: String,
    encode_frag: String,
    /// What naga called the uniform blocks and samplers after collapsing
    /// `@group`/`@binding`, which WebGL2 does not have. The front end looks its offsets
    /// up by name from the linked program, so these are for diagnostics rather than for
    /// binding — but a mismatch is otherwise a black canvas with no error.
    uniforms: Vec<String>,
}

/// §7.2's preview half: the product's WGSL, lowered to GLSL ES 3.00.
///
/// At startup rather than at build time. See `engine::glsl` — a generated `.frag` on
/// disk is an artefact that can go stale, and a stale one is the preview rendering a
/// different shader from the export, which is the drift §0 freezes against.
#[tauri::command]
fn shaders() -> Fallible<Shaders> {
    let (av, af) = glsl::lower_program(render::ADJUST_WGSL).map_err(err)?;
    let (ev, ef) = glsl::lower_program(render::ENCODE_WGSL).map_err(err)?;
    let mut uniforms = af.uniform_names.clone();
    uniforms.extend(ef.uniform_names.iter().cloned());
    Ok(Shaders {
        adjust_vert: av.source,
        adjust_frag: af.source,
        encode_vert: ev.source,
        encode_frag: ef.source,
        uniforms,
    })
}

// ---------------------------------------------------------------------- opening

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Opened {
    path: String,
    file_name: String,
    width: u32,
    height: u32,
    proxy_width: u32,
    proxy_height: u32,
    /// What §4 read the file as, so the UI can say which happened rather than implying
    /// a guess was a reading.
    source_space: String,
    colour_tag: String,
    bit_depth: u8,
    /// §4 discards it in v1 and says so out loud; the UI is where "says so" lands.
    gain_map: bool,
    alpha_composited: bool,
    orientation: u8,
    document: Document,
    /// Whether that document came off disk or was made fresh.
    from_sidecar: bool,
    /// §6.3's warnings, already worded. Empty for a fresh document.
    notices: Vec<String>,
    /// §6.3's read-only reason, if it opened that way. The UI disables saving and says
    /// this rather than failing at the moment somebody tries.
    read_only: Option<String>,
}

#[tauri::command]
fn open_image(
    state: tauri::State<'_, State>,
    path: String,
    viewport: u32,
) -> Fallible<Opened> {
    let path = PathBuf::from(path);
    let bytes = std::fs::read(&path).map_err(err)?;
    let decoded = decode::decode(&bytes).map_err(err)?;

    let full = decoded.image;
    let longest = Image::proxy_longest_edge(full.width().max(full.height()), viewport);
    let proxy = full.proxy(longest);

    // §6.1: a sidecar beside the photograph is this document. A source whose hash no
    // longer matches is §6.3's business, and `load` already has an opinion about it.
    let sidecar_path = sidecar::sidecar_path(&path);
    let (document, from_sidecar, notices, read_only) = match sidecar_path.exists() {
        true => {
            let loaded = sidecar::load(&sidecar_path).map_err(err)?;
            let notices = loaded.notices().iter().map(|n| n.to_string()).collect();
            let read_only = loaded.read_only().map(|r| r.to_string());
            // A read-only document still opens — §6.3 says "open read-only, explain,
            // offer export-as-new", so refusing here would be the one behaviour it
            // rules out. The owned copy is only needed to *save*, and that path is
            // closed separately.
            let document = loaded.document().clone();
            (document, true, notices, read_only)
        }
        false => (
            Document::new(Source {
                file: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                hash: sidecar::hash_source(&path).map_err(err)?,
                dimensions: [full.width(), full.height()],
                colorspace: match decoded.source_space.name {
                    n if n == DISPLAY_P3.name => ColorSpace::DisplayP3,
                    _ => ColorSpace::Srgb,
                },
                orientation: decoded.orientation,
            }),
            false,
            Vec::new(),
            None,
        ),
    };

    let opened = Opened {
        path: path.to_string_lossy().into_owned(),
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        width: full.width(),
        height: full.height(),
        proxy_width: proxy.width(),
        proxy_height: proxy.height(),
        source_space: decoded.source_space.name.to_string(),
        colour_tag: match decoded.tag {
            decode::ColourTag::Icc { bytes } => format!("ICC, {bytes} bytes"),
            decode::ColourTag::Nclx => "NCLX".into(),
            decode::ColourTag::SrgbChunk => "sRGB chunk".into(),
            decode::ColourTag::Untagged => "untagged".into(),
        },
        bit_depth: decoded.bit_depth,
        gain_map: decoded.gain_map.is_some(),
        alpha_composited: decoded.alpha_composited,
        orientation: decoded.orientation,
        document,
        from_sidecar,
        notices,
        read_only,
    };

    *state.session.lock().unwrap() = Some(Session {
        path,
        full,
        proxy,
        bytes,
    });
    Ok(opened)
}

/// The proxy as RGBA f16, which is what `gl.texImage2D` wants for an `RGBA16F` texture.
///
/// Raw bytes rather than JSON, and f16 rather than f32, because this is the one large
/// thing that crosses: 2 MP of RGBA f16 is 16 MB, the same array the GPU will hold.
/// Serialising it as numbers would be about 100 MB of text to produce, parse and throw
/// away.
///
/// Alpha is written as 1.0 rather than carried: the working buffer has no alpha channel
/// (§4, and the register), so the fourth channel exists here only because WebGL2's
/// float texture formats are RGBA.
#[tauri::command]
fn proxy_pixels(state: tauri::State<'_, State>) -> Fallible<Response> {
    let guard = state.session.lock().unwrap();
    let session = guard.as_ref().ok_or("no photograph is open")?;
    Ok(Response::new(rgba_f16(&session.proxy)))
}

fn rgba_f16(image: &Image) -> Vec<u8> {
    let one = f16::ONE.to_le_bytes();
    let src = image.pixels();
    let mut out = Vec::with_capacity(src.len() / 3 * 8);
    for rgb in src.chunks_exact(3) {
        for c in rgb {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&one);
    }
    out
}

// ---------------------------------------------------------------------- the plan

/// §6.2's compile, which happens here because §0 says it happens once and in Rust.
///
/// The front end executes the plan; it does not build one. Two compilers would render
/// two topologies and drift exactly as two shader sources would, with §12.2 then
/// comparing two *compilations* rather than two executions of one plan.
#[tauri::command]
fn compile(document: Document) -> Fallible<Graph> {
    graph::compile(&document).map_err(err)
}

/// Stage 13's uniform block for `space`, computed by the code that computes it for the
/// export (§16 #11, and the register).
#[tauri::command]
fn encode_uniform(space: String) -> Fallible<Response> {
    Ok(Response::new(render::encode_uniform_bytes(named(&space)?)))
}

fn named(space: &str) -> Fallible<&'static Space> {
    match space {
        "srgb" => Ok(&SRGB),
        "display-p3" => Ok(&DISPLAY_P3),
        other => Err(format!(
            "§4 handles sRGB and Display P3; `{other}` is a third space, which is a \
             decision rather than a value"
        )),
    }
}

// ------------------------------------------------------------------- persistence

/// §6.1's sidecar. The source file is never touched — that is invariant #1.
#[tauri::command]
fn save_sidecar(state: tauri::State<'_, State>, document: Document) -> Fallible<String> {
    let guard = state.session.lock().unwrap();
    let session = guard.as_ref().ok_or("no photograph is open")?;
    let at = sidecar::sidecar_path(&session.path);
    sidecar::save(&at, &document).map_err(err)?;
    Ok(at.to_string_lossy().into_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Exported {
    path: String,
    bytes: usize,
    width: u32,
    height: u32,
    millis: u64,
}

/// §6.1's `output` block, at full resolution, through the same shaders the preview ran.
#[tauri::command]
fn export_image(
    state: tauri::State<'_, State>,
    document: Document,
    path: String,
) -> Fallible<Exported> {
    let started = std::time::Instant::now();
    let guard = state.session.lock().unwrap();
    let session = guard.as_ref().ok_or("no photograph is open")?;

    let mut renderer = state.renderer.lock().unwrap();
    if renderer.is_none() {
        *renderer = Some(Renderer::new().map_err(err)?);
    }
    let renderer = renderer.as_ref().unwrap();

    let plan = graph::compile(&document).map_err(err)?;
    let rendered = renderer.render(&plan, &session.full).map_err(err)?;
    let metadata = export::SourceMetadata::from_jpeg(&session.bytes);
    let bytes = export::encode(&rendered, &document.output, &metadata).map_err(err)?;

    std::fs::write(&path, &bytes).map_err(err)?;
    Ok(Exported {
        path,
        bytes: bytes.len(),
        width: rendered.width(),
        height: rendered.height(),
        millis: started.elapsed().as_millis() as u64,
    })
}

/// Somewhere a failure can be read from outside the window.
///
/// Every error in the front end lands in a notice, and a notice is only visible to
/// whoever is looking at the window — which is nobody when the thing that failed is the
/// preview starting up, because then the window shows an empty canvas and says nothing
/// anybody can copy. This puts the same sentence on stderr.
#[tauri::command]
fn log(message: String, level: String) {
    // stderr for both. Rust block-buffers stdout when it is a pipe rather than a
    // terminal, so an informational line written with `println!` can still be sitting
    // in a buffer when the process is killed — which reads exactly like a front end
    // that never ran, and cost one diagnosis already.
    eprintln!("photodesk [{level}] {message}");
}

// ------------------------------------------------------------------------- main

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(State::default())
        .invoke_handler(tauri::generate_handler![
            shaders,
            open_image,
            proxy_pixels,
            compile,
            encode_uniform,
            save_sidecar,
            export_image,
            open_on_start,
            log,
        ])
        .setup(|app| {
            // A path on the command line opens straight into the editor, which is what
            // makes this testable from a shell and what `Open With` will use.
            if let Some(arg) = std::env::args().nth(1) {
                app.manage(OpenOnStart(Some(arg)));
            } else {
                app.manage(OpenOnStart(None));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("PhotoDesk failed to start");
}

/// A path given on the command line, for the front end to ask about once it is ready.
struct OpenOnStart(Option<String>);

#[tauri::command]
fn open_on_start(state: tauri::State<'_, OpenOnStart>) -> Option<String> {
    state.0.clone()
}
