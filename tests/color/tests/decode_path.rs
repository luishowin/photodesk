//! The decode path, cross-validated — §4's chain from a real container to the working
//! space.
//!
//! `src-tauri/tests/decode.rs` checks the decode path against files it builds itself.
//! That is the right place for the *mechanics* and the wrong place for the question
//! this file asks, which is whether the answers are **right** rather than merely
//! self-consistent. §2.2's standing discipline:
//!
//! > a colour suite that only agrees with itself is green and meaningless.
//!
//! So the ICC parser is checked against lcms2 reading the same bytes, and the whole
//! decode is checked against a container this harness encodes with known patches. Two
//! links, both tested — the same arrangement ΔE2000 and the matrix construction have
//! had since Spike B.
//!
//! This lives here rather than in `src-tauri/` because lcms2 lives here. The product
//! does not link it, deliberately: the thing that decides how a photograph is
//! interpreted should be code this project can read, and the harness should be able to
//! disagree with it.

use libheif_rs::{
    Channel, ColorProfileRaw, ColorSpace, CompressionFormat, EncoderQuality, HeifContext, Image,
    LibHeif, RgbChroma, color_profile_types,
};
use photodesk::engine::colour::{DISPLAY_P3, SRGB, Space};
use photodesk::engine::decode::{self, ColourTag};
use photodesk::engine::icc;
use photodesk_color::corpus::{self, COLORCHECKER_SRGB};
use photodesk_color::delta_e::{DeltaStats, ciede2000};
use photodesk_color::lab::encoded_to_lab;

/// An ICC profile for `space`, built by lcms2.
///
/// The point of using lcms2 to *write* what our parser *reads*: the bytes come from an
/// implementation that had no sight of the parser, so agreement is evidence about the
/// format rather than about one function talking to itself.
fn lcms_profile(space: &Space) -> Vec<u8> {
    use lcms2::{CIExyY, CIExyYTRIPLE, Profile, ToneCurve};
    let wp = CIExyY { x: space.white.x, y: space.white.y, Y: 1.0 };
    let primaries = CIExyYTRIPLE {
        Red: CIExyY { x: space.red.x, y: space.red.y, Y: 1.0 },
        Green: CIExyY { x: space.green.x, y: space.green.y, Y: 1.0 },
        Blue: CIExyY { x: space.blue.x, y: space.blue.y, Y: 1.0 },
    };
    let c = ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
        .expect("sRGB curve");
    Profile::new_rgb(&wp, &primaries, &[&c, &c, &c])
        .expect("build profile")
        .icc()
        .expect("serialise")
}

// ------------------------------------------------------ the parser, against lcms2

/// The product parses ICC profiles itself rather than linking lcms2. This is the test
/// that makes that defensible.
#[test]
fn our_icc_parser_agrees_with_lcms2() {
    for space in [&SRGB, &DISPLAY_P3] {
        let bytes = lcms_profile(space);

        // What lcms2 says the red colorant is, straight from the tag — the D50 value,
        // unadapted, exactly as stored.
        let theirs = lcms2::Profile::new_icc(&bytes).expect("lcms2 reads its own profile");
        let lcms_red = match theirs.read_tag(lcms2::TagSignature::RedColorantTag) {
            lcms2::Tag::CIEXYZ(xyz) => [xyz.X, xyz.Y, xyz.Z],
            other => panic!("no red colorant: {other:?}"),
        };

        // And what ours says, after adapting D50 → D65. Adapting lcms2's value the
        // same way is the only fair comparison: the two numbers are in different
        // spaces until one of them moves.
        let ours = icc::parse(&bytes).expect("our parser reads an lcms2 profile");
        let d65 = photodesk::engine::colour::D65;
        let d65_xyz = [d65.x / d65.y, 1.0, (1.0 - d65.x - d65.y) / d65.y];
        let adapted = icc::bradford([0.9642, 1.0, 0.8249], d65_xyz).apply([
            lcms_red[0] as f32,
            lcms_red[1] as f32,
            lcms_red[2] as f32,
        ]);
        let m = &ours.to_xyz_d65.0;
        let mine = [m[0][0], m[1][0], m[2][0]];

        let worst = (0..3)
            .map(|i| (mine[i] - adapted[i] as f64).abs())
            .fold(0.0, f64::max);
        println!(
            "{}: lcms2 red colorant (D50) {lcms_red:.4?}\n  \
             adapted to D65 {adapted:.4?} vs ours {mine:.4?}  worst {worst:.2e}",
            space.name
        );
        assert!(
            worst < 1e-6,
            "our ICC parser disagrees with lcms2 on {}'s red colorant by {worst:.3e}",
            space.name
        );

        // And the classification, which is the answer that actually matters.
        assert_eq!(ours.classify().expect("classify").name, space.name);
    }
}

/// The colorants our parser recovers are the ones `colour.rs` derives from
/// chromaticities — so the ICC round trip does not quietly move the space.
///
/// This closes a loop the earlier tests leave open: `our_icc_parser_agrees_with_lcms2`
/// shows we read the same bytes lcms2 wrote, and this shows those bytes describe the
/// space we think they do.
#[test]
fn a_profile_round_trips_back_to_the_space_it_came_from() {
    for space in [&SRGB, &DISPLAY_P3] {
        let parsed = icc::parse(&lcms_profile(space)).expect("parse");
        let want = space.to_xyz();
        let worst = (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| (parsed.to_xyz_d65.0[i][j] - want.0[i][j]).abs())
            .fold(0.0, f64::max);
        println!("{}: worst colorant difference after an ICC round trip {worst:.2e}", space.name);
        assert!(
            worst < 1e-4,
            "{} does not survive being written to ICC and read back: {worst:.3e}. \
             s15Fixed16 quantises at 1.5e-5 and Bradford round-trips at ~1e-7, so \
             anything larger is the parser",
            space.name
        );
    }
}

/// The mirror of the test above: lcms2 reading a profile **we** wrote.
///
/// §4's chain ends "→ ICC-tagged file", and the tag is written in-tree for the same
/// reason it is read in-tree. `export.rs`'s own test shows our parser reads it back;
/// that is self-agreement and worth little on its own. This asks an implementation
/// that has never seen the writer whether the file says what we meant.
#[test]
fn lcms2_reads_the_profiles_we_write() {
    for space in [&SRGB, &DISPLAY_P3] {
        let bytes = icc::write(space);
        let theirs = lcms2::Profile::new_icc(&bytes)
            .unwrap_or_else(|e| panic!("lcms2 will not open our {} profile: {e:?}", space.name));

        let red = match theirs.read_tag(lcms2::TagSignature::RedColorantTag) {
            lcms2::Tag::CIEXYZ(xyz) => [xyz.X, xyz.Y, xyz.Z],
            other => panic!("{}: lcms2 found no red colorant, only {other:?}", space.name),
        };
        // Ours, written to the same tag, read back by us. The two paths meet at the
        // bytes rather than at a shared function.
        let ours = icc::parse(&bytes).expect("parse").to_xyz_d65.0;
        let d65 = photodesk::engine::colour::D65;
        let d65_xyz = [d65.x / d65.y, 1.0, (1.0 - d65.x - d65.y) / d65.y];
        let theirs_d65 = icc::bradford([0.9642, 1.0, 0.8249], d65_xyz).apply([
            red[0] as f32,
            red[1] as f32,
            red[2] as f32,
        ]);
        let worst = (0..3)
            .map(|i| (ours[i][0] - theirs_d65[i] as f64).abs())
            .fold(0.0, f64::max);
        println!(
            "{} ({} bytes): lcms2 reads red {red:.4?} (D50); adapted {theirs_d65:.4?} \
             against ours; worst {worst:.2e}",
            space.name,
            bytes.len()
        );
        assert!(worst < 1e-6, "{}: {worst:.3e}", space.name);

        // And the thing that actually matters: a transform built from our profile
        // agrees with one built from lcms2's own idea of the same space.
        let curve =
            lcms2::ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
                .expect("curve");
        let reference = lcms2::Profile::new_rgb(
            &lcms2::CIExyY { x: space.white.x, y: space.white.y, Y: 1.0 },
            &lcms2::CIExyYTRIPLE {
                Red: lcms2::CIExyY { x: space.red.x, y: space.red.y, Y: 1.0 },
                Green: lcms2::CIExyY { x: space.green.x, y: space.green.y, Y: 1.0 },
                Blue: lcms2::CIExyY { x: space.blue.x, y: space.blue.y, Y: 1.0 },
            },
            &[&curve, &curve, &curve],
        )
        .expect("reference profile");
        let t: lcms2::Transform<[f64; 3], [f64; 3]> = lcms2::Transform::new(
            &theirs,
            lcms2::PixelFormat::RGB_DBL,
            &reference,
            lcms2::PixelFormat::RGB_DBL,
            lcms2::Intent::RelativeColorimetric,
        )
        .expect("ours -> theirs");

        let input: Vec<[f64; 3]> = COLORCHECKER_SRGB
            .iter()
            .map(|c| [c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0])
            .collect();
        let mut out = vec![[0.0f64; 3]; input.len()];
        t.transform_pixels(&input, &mut out);
        let deltas: Vec<f64> = input
            .iter()
            .zip(&out)
            .map(|(a, b)| {
                ciede2000(
                    encoded_to_lab([a[0] as f32, a[1] as f32, a[2] as f32], space),
                    encoded_to_lab([b[0] as f32, b[1] as f32, b[2] as f32], space),
                )
            })
            .collect();
        let stats = DeltaStats::from(&deltas);
        println!("  our profile -> lcms2's own {}: {stats}", space.name);
        assert!(
            stats.max < 0.01,
            "a file tagged with our profile does not describe {}: {stats}",
            space.name
        );
    }
}

// ------------------------------------------------- the whole decode, against a chart

/// §4's chain end to end, on a container this harness encodes so the answer is known.
///
/// The 24 ColorChecker patches go in as Display P3 codes and come out as linear P3
/// f16; the assertion is that they are still the same colours. That is §2.2's test 1
/// — round-trip identity — asked of the *product's* decoder rather than of a model of
/// it, which is the thing that only became possible when the colour code moved out of
/// this crate.
#[test]
fn a_real_container_decodes_to_the_colours_it_went_in_as() {
    const W: u32 = 24;
    const H: u32 = 16;
    let lh = LibHeif::new();

    // Every container this machine can write, because §12.1's thresholds will differ
    // per codec and the YCbCr floor (§3) is one of the reasons.
    let formats = [
        ("uncompressed", CompressionFormat::Uncompressed),
        ("AV1 (AVIF)", CompressionFormat::Av1),
        ("HEVC (HEIC)", CompressionFormat::Hevc),
    ];
    let available: Vec<_> = formats
        .iter()
        .filter(|(_, f)| !lh.encoder_descriptors(4, Some(*f), None).is_empty())
        .collect();
    assert!(!available.is_empty(), "libheif can encode nothing here");

    // A 6×4 chart of the 24 patches, four pixels each way, re-encoded as Display P3.
    let patches: Vec<[u8; 3]> = corpus::colorchecker_display_p3()
        .iter()
        .map(|p| {
            let q = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            [q(p[0]), q(p[1]), q(p[2])]
        })
        .collect();
    let at = |x: u32, y: u32| patches[((y / 4) * 6 + (x / 4)) as usize];

    for (label, format) in available {
        let mut img = Image::new(W, H, ColorSpace::Rgb(RgbChroma::Rgb)).expect("image");
        img.create_plane(Channel::Interleaved, W, H, 8).expect("plane");
        {
            let mut planes = img.planes_mut();
            let plane = planes.interleaved.as_mut().expect("interleaved");
            for y in 0..H {
                for x in 0..W {
                    let p = at(x, y);
                    let i = y as usize * plane.stride + x as usize * 3;
                    plane.data[i..i + 3].copy_from_slice(&p);
                }
            }
        }
        img.set_color_profile_raw(&ColorProfileRaw::new(
            color_profile_types::PROF,
            lcms_profile(&DISPLAY_P3),
        ))
        .expect("attach ICC");

        let mut ctx = HeifContext::new().expect("context");
        let mut encoder = lh.encoder_for_format(*format).expect("encoder");
        let _ = encoder.set_quality(EncoderQuality::LossLess);
        ctx.encode_image(&img, &mut encoder, None).expect("encode");
        let bytes = ctx.write_to_bytes().expect("serialise");

        // …and now the product opens it, knowing nothing about how it was made.
        let decoded = decode::decode(&bytes).expect("the product decodes it");
        assert_eq!(decoded.source_space.name, DISPLAY_P3.name, "{label}: wrong space");
        assert!(matches!(decoded.tag, ColourTag::Icc { .. }), "{label}: profile not found");
        assert_eq!((decoded.image.width(), decoded.image.height()), (W, H));

        // Compare in Lab: what went in, against what the working-space value encodes
        // back to. Sampled at patch centres, away from the block edges where a DCT
        // codec rings.
        let deltas: Vec<f64> = (0..24)
            .map(|i| {
                let (x, y) = ((i % 6) * 4 + 2, (i / 6) * 4 + 2);
                let want = patches[i as usize];
                let got = decoded.image.pixel(x, y);
                let encoded = [
                    DISPLAY_P3.transfer.from_linear(got[0]),
                    DISPLAY_P3.transfer.from_linear(got[1]),
                    DISPLAY_P3.transfer.from_linear(got[2]),
                ];
                ciede2000(
                    encoded_to_lab(
                        [
                            want[0] as f32 / 255.0,
                            want[1] as f32 / 255.0,
                            want[2] as f32 / 255.0,
                        ],
                        &DISPLAY_P3,
                    ),
                    encoded_to_lab(encoded, &DISPLAY_P3),
                )
            })
            .collect();
        let stats = DeltaStats::from(&deltas);
        println!("{label:<14} decode -> working space -> encode: {stats}");

        // The bar is §3's measured YCbCr floor plus room, and it is different for the
        // uncompressed container because that one does not go through YCbCr at all.
        let bar = if matches!(format, CompressionFormat::Uncompressed) { 0.2 } else { 1.5 };
        assert!(
            stats.max < bar,
            "{label}: the product's decode moved the chart by {stats}, past {bar}. \
             §3 measured the RGB↔YCbCr conversion floor at ΔE 0.9041 max, so a YCbCr \
             container is expected to spend some of this budget and an uncompressed \
             one is not"
        );
    }
}

/// §16 #13's error has to be reachable, or it is a message nobody will ever read.
///
/// The codec *is* installed here, so the missing-codec branch cannot be provoked by
/// asking for HEVC. What can be checked is the other half of the same claim: that the
/// decoder consults the codec list at all, and that a container whose codec is present
/// gets past it.
#[test]
fn the_codec_check_runs_before_the_decode() {
    let lh = LibHeif::new();
    let hevc = !lh.decoder_descriptors(1, Some(CompressionFormat::Hevc)).is_empty();
    println!("HEVC decoder present: {hevc}");
    assert!(
        hevc,
        "no HEVC decoder — install RPM Fusion's libheif-freeworld. §16 #13 exists so \
         this is a sentence rather than a mystery"
    );

    // A truncated HEIF: the codec is available, so this must fail as a *broken file*
    // rather than as a missing codec. Telling those two apart is the whole of §16 #13.
    let mut truncated = vec![0, 0, 0, 0x18];
    truncated.extend_from_slice(b"ftypheic");
    truncated.extend_from_slice(b"\0\0\0\0heicmif1");
    let err = decode::decode(&truncated).expect_err("a truncated container");
    println!("{err}");
    assert!(
        matches!(err, decode::DecodeError::Broken(_)),
        "a truncated file was reported as a missing codec: {err}"
    );
}

// ----------------------------------------------------------------- real photographs

/// The corpus, when there is one. Everything above is synthetic by construction; this
/// is the only test in the file that opens something a camera made.
#[test]
fn the_product_opens_a_real_photograph() {
    let dir = corpus::corpus_dir();
    let mut opened = 0;
    for path in [corpus::find_heic(&dir), corpus::find_jpeg(&dir)].into_iter().flatten() {
        let decoded = match decode::open(&path) {
            Ok(d) => d,
            Err(e) => panic!("{}: {e}", path.display()),
        };
        println!(
            "{}\n  {:?}  {}  {} bpc  tag {:?}{}",
            path.file_name().unwrap().to_string_lossy(),
            decoded.image,
            decoded.source_space.name,
            decoded.bit_depth,
            decoded.tag,
            decoded
                .gain_map
                .as_ref()
                .map(|g| format!("  gain map {}×{} ({}), skipped per §4", g.width, g.height, g.kind))
                .unwrap_or_default()
        );
        // §4 measured both containers as Display P3 with the same 536-byte profile.
        assert!(decoded.image.width() > 0 && decoded.image.height() > 0);
        opened += 1;

        // And §7.1's proxy, at the size a 1080p viewport asks for.
        let edge = photodesk::engine::image::Image::proxy_longest_edge(
            decoded.image.width().max(decoded.image.height()),
            1920,
        );
        let proxy = decoded.image.proxy(edge);
        println!("  proxy for a 1080p viewport: {proxy:?}");
        assert!(proxy.width().max(proxy.height()) <= edge);
    }
    if opened == 0 {
        println!(
            "SKIP: no photograph in {}. Set PHOTODESK_CORPUS_DIR. This test would open \
             a real file through the product's own decode path and report what §4 read \
             out of it.",
            dir.display()
        );
    }
}
