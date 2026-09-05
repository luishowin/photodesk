//! §2.2's last blocked corpus item: a real iPhone HEIC, gain map and all.
//!
//! Everything else in this suite is synthetic, deliberately — exact known values are
//! what prove a transform. What synthetic corpora cannot prove is that a *real* file
//! from the device §1 names as the native subject has the shape we assumed: that the
//! profile is where we look for it, that the gain map is an auxiliary image we can
//! recognise and skip, and that the SDR base is what decodes by default.
//!
//! **The photographs are not in the repository and must not be.** They are personal
//! files, §12.1 puts corpus binaries behind git-lfs, and a test that only runs where
//! the data is happens to be the honest arrangement here. Point `PHOTODESK_CORPUS_DIR`
//! at a directory of real photographs, or drop them in `~/Downloads`; absent them this
//! test skips and says what it would have checked.
//!
//! This test reads structure and colour. It does not print EXIF values — §6.1's export
//! default is `keep-minus-gps`, and a test log is not the place to leak a location.

use libheif_rs::{ColorProfile, ColorSpace, HeifContext, ImageHandle, LibHeif, RgbChroma};
use photodesk_color::colour::{DISPLAY_P3, SRGB, Space, encoded_to_lab};
use photodesk_color::delta_e::{DeltaStats, ciede2000};
use photodesk_color::working::{Pipeline, Precision};
use std::path::{Path, PathBuf};

/// Apple stores the HDR gain map as an auxiliary image under this URN.
const APPLE_GAIN_MAP: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";

fn corpus_dir() -> PathBuf {
    std::env::var("PHOTODESK_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads")
        })
}

fn find_heic(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

fn describe(handle: &ImageHandle, indent: &str) {
    println!(
        "{indent}{}x{}  luma {} bpp, chroma {} bpp, alpha: {}",
        handle.width(),
        handle.height(),
        handle.luma_bits_per_pixel(),
        handle.chroma_bits_per_pixel(),
        handle.has_alpha_channel()
    );
}

#[test]
fn real_iphone_heic_structure_matches_what_section_4_assumes() {
    let dir = corpus_dir();
    let Some(path) = find_heic(&dir) else {
        println!(
            "SKIP: no .heic/.heif in {}. Set PHOTODESK_CORPUS_DIR to a directory of real \n\
             photographs. This test would check: the embedded profile is readable, the \n\
             HDR gain map is present as a recognisable auxiliary image, the SDR base is \n\
             what decodes by default, and §4's discard is a decision rather than an \n\
             accident.",
            dir.display()
        );
        return;
    };
    println!("file: {}", path.file_name().unwrap().to_string_lossy());
    println!("size: {} bytes", std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));

    let lh = LibHeif::new();
    let ctx = HeifContext::read_from_file(path.to_str().expect("utf-8 path"))
        .expect("libheif could not open the file");
    let handle = ctx.primary_image_handle().expect("primary image handle");

    println!("\nprimary image");
    describe(&handle, "  ");
    println!("  top-level images in container: {}", ctx.image_ids().len());

    // ---- the colour tag ----------------------------------------------------
    println!("\ncolour");
    let icc = handle.color_profile_raw();
    let nclx = handle.color_profile_nclx();
    match (&icc, &nclx) {
        (Some(p), _) => println!(
            "  ICC: {} bytes, type {:?}",
            p.data.len(),
            p.profile_type()
        ),
        (None, Some(n)) => println!(
            "  no ICC; NCLX primaries={:?} transfer={:?} matrix={:?}",
            n.color_primaries(),
            n.transfer_characteristics(),
            n.matrix_coefficients()
        ),
        (None, None) => println!("  no colour information at all — §4 says assume sRGB"),
    }

    // An iPhone photograph is Display P3. If neither tag says so, §4's whole premise
    // (take the manufacturer's rendering as the starting point) has nothing to read.
    let source_space: &Space = if let Some(p) = &icc {
        let parsed = lcms2::Profile::new_icc(&p.data).expect("embedded ICC does not parse");
        let red = match parsed.read_tag(lcms2::TagSignature::RedColorantTag) {
            lcms2::Tag::CIEXYZ(xyz) => *xyz,
            other => panic!("embedded profile has no red colorant: {other:?}"),
        };
        println!("  red colorant XYZ: ({:.4}, {:.4}, {:.4})", red.X, red.Y, red.Z);
        // sRGB's red lands near X 0.4361, Display P3's near 0.5151.
        if red.X > 0.48 {
            println!("  -> Display P3");
            &DISPLAY_P3
        } else {
            println!("  -> sRGB");
            &SRGB
        }
    } else if let Some(n) = &nclx {
        println!("  -> NCLX only; primaries {:?}", n.color_primaries());
        &DISPLAY_P3
    } else {
        &SRGB
    };

    // ---- the gain map ------------------------------------------------------
    println!("\nauxiliary images");
    let aux = handle.auxiliary_images(None);
    if aux.is_empty() {
        println!("  none");
    }
    let mut gain_map = None;
    for a in &aux {
        let t = a.auxiliary_type().unwrap_or_default();
        print!("  {t}\n    ");
        describe(a, "");
        if t == APPLE_GAIN_MAP || t.contains("hdrgainmap") || t.contains("gainmap") {
            gain_map = Some(t);
        }
    }

    println!("\nmetadata blocks");
    for m in handle.all_metadata() {
        // Types and sizes only. The values are the user's, and §6.1's export default
        // is keep-minus-gps for a reason.
        println!(
            "  {} ({} bytes){}",
            m.item_type,
            m.raw_data.len(),
            if m.content_type.is_empty() {
                String::new()
            } else {
                format!(", {}", m.content_type)
            }
        );
    }

    // ---- §4's contract -----------------------------------------------------
    println!("\n§4's v1 contract");
    match &gain_map {
        Some(t) => println!(
            "  gain map PRESENT as `{t}` — v1 decodes the SDR base and discards this. \n\
             §4 requires that to be a decision rather than an accident, and the point of \n\
             this assertion is that we can *see* it in order to skip it deliberately."
        ),
        None => println!(
            "  no gain map in this file. §4's discard is untestable here; it is not \n\
             wrong, just unexercised. A photo from a modern iPhone in HDR would carry one."
        ),
    }

    // The default decode has to give the SDR base, unmodified by the gain map. If
    // libheif ever started applying it automatically, an image would silently change
    // appearance between versions — which is exactly the class of thing §4 wrote its
    // gain-map paragraph to prevent.
    let decoded = lh
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
        .expect("default decode of the primary image failed");
    assert_eq!(
        (decoded.width(), decoded.height()),
        (handle.width(), handle.height()),
        "the default decode is not the primary image at its own size"
    );
    let planes = decoded.planes();
    let plane = planes.interleaved.expect("interleaved RGB plane");
    println!("  default decode: {}x{}, stride {}", decoded.width(), decoded.height(), plane.stride);

    // ---- round-trip the real pixels through the working space --------------
    // Sampled on a coarse grid: this is a correctness check on the transform, not an
    // image comparison, and a few thousand samples say as much as two million.
    let pipeline = Pipeline::new(Precision::F16, 5);
    let (w, h) = (decoded.width(), decoded.height());
    let step = (w.min(h) / 48).max(1);
    let mut deltas = Vec::new();
    let mut clipped = 0usize;
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let o = y as usize * plane.stride + (x * 3) as usize;
            if o + 2 >= plane.data.len() {
                continue;
            }
            let px = [plane.data[o], plane.data[o + 1], plane.data[o + 2]];
            let out = pipeline.round_trip_u8(px, source_space, source_space);
            if px.iter().any(|c| *c == 0 || *c == 255) {
                clipped += 1;
            }
            let a = encoded_to_lab(
                [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0],
                source_space,
            );
            let b = encoded_to_lab(
                [out[0] as f32 / 255.0, out[1] as f32 / 255.0, out[2] as f32 / 255.0],
                source_space,
            );
            deltas.push(ciede2000(a, b));
        }
    }
    let stats = DeltaStats::from(&deltas);
    println!(
        "\nround trip through linear P3 f16, real pixels: {stats}\n  \
         ({} of {} samples touch a clipping bound)",
        clipped,
        stats.count
    );
    assert!(
        stats.count > 500,
        "only {} samples taken — the grid stride is wrong",
        stats.count
    );
    assert!(
        stats.max < 1.0,
        "a real photograph does not survive the working space: {stats}"
    );

    // Export to sRGB has to stay sane too: this is the §14 v0.1 path end to end.
    let export: Vec<f64> = (0..h)
        .step_by(step as usize)
        .flat_map(|y| {
            (0..w).step_by(step as usize).filter_map(move |x| {
                let o = y as usize * plane.stride + (x * 3) as usize;
                (o + 2 < plane.data.len()).then(|| {
                    let px = [plane.data[o], plane.data[o + 1], plane.data[o + 2]];
                    let out = pipeline.round_trip_u8(px, source_space, &SRGB);
                    let a = encoded_to_lab(
                        [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0],
                        source_space,
                    );
                    let b = encoded_to_lab(
                        [out[0] as f32 / 255.0, out[1] as f32 / 255.0, out[2] as f32 / 255.0],
                        &SRGB,
                    );
                    ciede2000(a, b)
                })
            })
        })
        .collect();
    let export_stats = DeltaStats::from(&export);
    println!(
        "export to sRGB, same pixels: {export_stats}\n  \
         (not an error — P3 content outside sRGB genuinely moves when gamut-mapped)"
    );
}


// ---------------------------------------------------------------------------
// §1 names three native subjects: an iPhone HEIF, a JPEG, and a screenshot. The
// HEIC is covered above. A JPEG carries its profile somewhere completely
// different — APP2 segments, chunked, with a 14-byte header on each — and
// "we read the ICC" is a claim about both containers or neither.

/// Reassemble an ICC profile from a JPEG's APP2 `ICC_PROFILE` segments.
///
/// Hand-parsed rather than pulled from a decoder: it is thirty lines, it needs no
/// dependency, and the chunking is the part that goes wrong. A profile over 64 KB is
/// split across numbered chunks that have to be concatenated in order — miss that and
/// large profiles silently truncate into something that still parses.
fn jpeg_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"ICC_PROFILE\0";
    let mut i = 2usize; // skip SOI
    let mut chunks: Vec<(u8, &[u8])> = Vec::new();
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let marker = bytes[i + 1];
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            break; // start of scan; no more metadata
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let seg = bytes.get(i + 4..i + 2 + len)?;
        if marker == 0xE2 && seg.len() > TAG.len() + 2 && seg.starts_with(TAG) {
            let seq = seg[TAG.len()];
            chunks.push((seq, &seg[TAG.len() + 2..]));
        }
        i += 2 + len;
    }
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|(seq, _)| *seq);
    Some(chunks.into_iter().flat_map(|(_, d)| d.iter().copied()).collect())
}

fn find_jpeg(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
        })
        .collect();
    // Prefer the largest: a camera original rather than a downloaded thumbnail.
    found.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));
    found.pop()
}

#[test]
fn real_jpeg_carries_a_readable_profile() {
    let dir = corpus_dir();
    let Some(path) = find_jpeg(&dir) else {
        println!(
            "SKIP: no .jpg/.jpeg in {}. This test would check that a JPEG's ICC is \
             reassembled correctly from its APP2 chunks.",
            dir.display()
        );
        return;
    };
    let bytes = std::fs::read(&path).expect("read jpeg");
    println!(
        "file: {} ({} bytes)",
        path.file_name().unwrap().to_string_lossy(),
        bytes.len()
    );

    match jpeg_icc(&bytes) {
        Some(icc) => {
            println!("APP2 ICC: {} bytes", icc.len());
            let parsed = lcms2::Profile::new_icc(&icc).expect("APP2 ICC does not parse");
            let red = match parsed.read_tag(lcms2::TagSignature::RedColorantTag) {
                lcms2::Tag::CIEXYZ(xyz) => *xyz,
                other => panic!("no red colorant: {other:?}"),
            };
            println!("  red colorant XYZ: ({:.4}, {:.4}, {:.4})", red.X, red.Y, red.Z);
            println!(
                "  -> {}",
                if red.X > 0.48 { "Display P3" } else { "sRGB or similar" }
            );
            // Whatever it is, it has to be one of the two spaces §4 handles. A profile
            // we can parse but not classify is worse than no profile, because the app
            // would proceed confidently down the wrong path.
            assert!(
                (0.40..0.56).contains(&red.X),
                "red primary at X {:.4} is neither sRGB's 0.4361 nor P3's 0.5151 — §4 \
                 handles two spaces and this file is in a third",
                red.X
            );
        }
        None => println!(
            "no APP2 ICC in this JPEG. §4 says assume sRGB, and this is the case that \
             exercises it — an untagged file is the common one, not the exception."
        ),
    }
}
