//! Write a HEIC, so that §1's native subject can be opened by hand.
//!
//! `heif_icc.rs` already proves the decode path reads a real container correctly, and
//! it builds its own containers to do it. What it cannot do is hand one to a person:
//! the file lives inside a test and dies with it. Every photograph the *application*
//! has opened so far has been a JPEG, which leaves the format §1 is actually about
//! exercised only by the half of the system that has no window.
//!
//! So this is the same construction, kept. It needs an HEVC encoder — on Fedora that
//! is `libheif-freeworld` — and says so by name when there is none, because a missing
//! codec is not a broken tool (§16 #13, and the same sentence the decoder uses).
//!
//! ```
//! cargo run -p photodesk-color --example make-heic -- /tmp/scene.heic
//! cargo run -p photodesk-app -- /tmp/scene.heic
//! ```
//!
//! The picture is synthetic and says so. It is not a substitute for a photograph off a
//! phone — no EXIF, no gain map, 8-bit — and the things it is built to have are the
//! ones the six sliders act on: a highlight with detail inside it, a shadow with
//! detail inside it, a smooth gradient that will band if something quantises, and
//! saturated patches near the edge of P3 that will move visibly when the gamut policy
//! or the temperature does.

use libheif_rs::{
    Channel, ColorPrimaries, ColorProfileNCLX, ColorProfileRaw, ColorSpace, CompressionFormat,
    EncoderQuality, EncodingOptions, HeifContext, Image, LibHeif, RgbChroma, color_profile_types,
};
use photodesk::engine::colour::DISPLAY_P3;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut out = None;
    let mut nclx = false;
    let mut width = 4032u32;
    let mut height = 3024u32;
    let mut quality = 90u8;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            // Which of §4's two ways a HEIF can state its colour. Both reach the
            // decoder; they are different branches and only one of them can be the
            // default, so the other needs a flag to ever run.
            "--nclx" => nclx = true,
            "--icc" => nclx = false,
            "--size" => {
                let v = args.next().unwrap_or_default();
                let (w, h) = v.split_once('x').unwrap_or_else(|| die("--size wants WxH"));
                width = w.parse().unwrap_or_else(|_| die("--size wants WxH"));
                height = h.parse().unwrap_or_else(|_| die("--size wants WxH"));
            }
            "--quality" => {
                quality = args.next().and_then(|q| q.parse().ok()).unwrap_or_else(|| {
                    die("--quality wants 0-100")
                });
            }
            other if out.is_none() => out = Some(other.to_string()),
            other => die(&format!("unexpected argument `{other}`")),
        }
    }
    let out = out.unwrap_or_else(|| die("usage: make-heic <out.heic> [--nclx|--icc] [--size WxH] [--quality N]"));

    let lh = LibHeif::new();
    // Asked before anything is built, and named the way the decoder names it, so that
    // a machine without the codec gets the sentence it can act on rather than a
    // failure from four frames deeper.
    if lh.encoder_descriptors(1, Some(CompressionFormat::Hevc), None).is_empty() {
        die(
            "this machine has no HEVC encoder, so it cannot write a HEIC. \
             Fedora ships libheif without one on patent grounds; install libheif-freeworld.",
        );
    }

    let rgb = scene(width, height);

    let mut img = Image::new(width, height, ColorSpace::Rgb(RgbChroma::Rgb)).expect("create image");
    img.create_plane(Channel::Interleaved, width, height, 8).expect("create plane");
    {
        let mut planes = img.planes_mut();
        let plane = planes.interleaved.as_mut().expect("interleaved plane");
        // libheif pads its rows, which is the whole reason `to_working_space` takes a
        // described surface rather than a pointer. Written row by row for the same
        // reason it is read row by row.
        let row = (width * 3) as usize;
        for y in 0..height as usize {
            let dst = y * plane.stride;
            plane.data[dst..dst + row].copy_from_slice(&rgb[y * row..y * row + row]);
        }
    }

    // libheif suppresses the NCLX `colr` box by default — `macOS_compatibility_
    // workaround_no_nclx_profile` is on, because macOS mishandles it. Left alone, an
    // `--nclx` file comes out with *no* colour box at all, and the decoder correctly
    // calls it untagged: the flag has to be turned off for the branch to exist. Found
    // by walking the container's boxes in Python, which is the only thing here that
    // did not go through libheif.
    let mut options = EncodingOptions::new().expect("encoding options");
    options.set_mac_os_compatibility_workaround_no_nclx_profile(false);

    if nclx {
        let mut profile = ColorProfileNCLX::new().expect("allocate NCLX");
        // SMPTE EG 432-1 is Display P3's primaries — the code an iPhone writes.
        profile.set_color_primaries(ColorPrimaries::SMPTE_EG_432_1);
        img.set_color_profile_nclx(&profile).expect("attach NCLX");
    } else {
        img.set_color_profile_raw(&ColorProfileRaw::new(color_profile_types::PROF, display_p3_icc()))
            .expect("attach ICC");
    }

    let mut encoder = lh.encoder_for_format(CompressionFormat::Hevc).expect("HEVC encoder");
    encoder.set_quality(EncoderQuality::Lossy(quality)).expect("set quality");

    let mut ctx = HeifContext::new().expect("context");
    ctx.encode_image(&img, &mut encoder, Some(options)).expect("encode");
    let bytes = ctx.write_to_bytes().expect("serialise");
    std::fs::write(&out, &bytes).unwrap_or_else(|e| die(&format!("writing {out}: {e}")));

    println!(
        "{out}: {width}×{height}, {} bytes, {}, HEVC q{quality} ({})",
        bytes.len(),
        if nclx { "NCLX Display P3" } else { "ICC Display P3" },
        encoder.name(),
    );
}

/// A Display P3 ICC profile, built by lcms2 — which is not the parser that will read
/// it back. Same arrangement as the rest of this crate, and the reason lcms2 is a
/// dev-dependency here and nowhere near the product.
fn display_p3_icc() -> Vec<u8> {
    use lcms2::{CIExyY, CIExyYTRIPLE, Profile, ToneCurve};
    let wp = CIExyY { x: DISPLAY_P3.white.x, y: DISPLAY_P3.white.y, Y: 1.0 };
    let primaries = CIExyYTRIPLE {
        Red: CIExyY { x: DISPLAY_P3.red.x, y: DISPLAY_P3.red.y, Y: 1.0 },
        Green: CIExyY { x: DISPLAY_P3.green.x, y: DISPLAY_P3.green.y, Y: 1.0 },
        Blue: CIExyY { x: DISPLAY_P3.blue.x, y: DISPLAY_P3.blue.y, Y: 1.0 },
    };
    let c = ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
        .expect("sRGB curve");
    Profile::new_rgb(&wp, &primaries, &[&c, &c, &c])
        .expect("build P3 profile")
        .icc()
        .expect("serialise ICC")
}

/// Display-encoded Display P3, interleaved RGB8.
///
/// Deliberately not a photograph. Each region exists to make one slider legible:
/// the gradient for banding, the disc for `highlights`, the foreground for `shadows`
/// and `blacks`, the patches for `temperature` and the gamut policy.
fn scene(width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 3) as usize];
    let (w, h) = (width as f32, height as f32);
    // Where the ground starts, and where the patch strip sits inside it.
    let horizon = h * 0.62;
    let sun = (w * 0.72, h * 0.24, h * 0.10);

    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f32, y as f32);
            let mut c = if fy < horizon {
                // Sky: warm at the horizon, blue at the top. Smooth over ~1900 rows,
                // which is far more steps than 8 bits has — so any extra quantisation
                // in the pipeline shows up here as bands.
                let t = fy / horizon;
                [
                    lerp(0.24, 0.99, t),
                    lerp(0.42, 0.86, t),
                    lerp(0.86, 0.66, t),
                ]
            } else {
                // Ground: dark, with texture that lives entirely in the bottom two
                // stops. `shadows` has something to lift; `blacks` has somewhere to go.
                let t = (fy - horizon) / (h - horizon);
                let grain = ((fx * 0.11).sin() * (fy * 0.07).cos()) * 0.5 + 0.5;
                let v = lerp(0.10, 0.02, t) + grain * 0.045;
                [v * 1.05, v, v * 0.92]
            };

            // A sun with a soft edge: clipped in the middle, recoverable at the rim,
            // which is the shape `highlights` is for.
            let d = ((fx - sun.0).powi(2) + (fy - sun.1).powi(2)).sqrt() / sun.2;
            if d < 2.4 {
                let glow = (1.0 - (d / 2.4)).powf(2.6);
                for (i, k) in [1.0, 0.97, 0.88].into_iter().enumerate() {
                    c[i] = (c[i] + glow * k * 1.35).min(1.0);
                }
            }

            // Six saturated patches along the bottom, at the edge of P3 where sRGB
            // cannot follow — which is what makes the gamut policy visible at all.
            let strip = h * 0.90;
            let band = h * 0.055;
            if fy >= strip && fy < strip + band {
                let i = ((fx / w) * 6.0) as usize;
                let inset = fx % (w / 6.0);
                if inset > w / 60.0 && inset < w / 6.0 - w / 60.0 {
                    c = PATCHES[i.min(5)];
                }
            }
            let o = ((y * width + x) * 3) as usize;
            for i in 0..3 {
                out[o + i] = (c[i].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// Near the P3 primaries and secondaries. In sRGB every one of these is outside the
/// cube, so what the export does with them is a choice rather than a rounding.
const PATCHES: [[f32; 3]; 6] = [
    [0.98, 0.06, 0.06],
    [0.98, 0.62, 0.04],
    [0.92, 0.95, 0.08],
    [0.06, 0.94, 0.22],
    [0.05, 0.68, 0.96],
    [0.62, 0.10, 0.94],
];

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn die(message: &str) -> ! {
    eprintln!("make-heic: {message}");
    std::process::exit(1);
}
