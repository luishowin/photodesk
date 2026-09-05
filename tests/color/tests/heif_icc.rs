//! §2.2's blocked corpus item, unblocked: ICC extraction from a real container.
//!
//! The synthetic patches in `spike_b.rs` prove the matrices are right. They cannot
//! prove we read the tag that says *which* matrices to use — and Spike A found that
//! RapidRAW gets exactly that wrong, silently, for every Display P3 file it opens
//! (`FORK-AUDIT.md`). This is the test that would catch it.
//!
//! The container is built here rather than shipped as a fixture: a committed binary is
//! opaque, needs git-lfs (§12.1), and cannot say what it contains. Code can.

use libheif_rs::{
    Channel, ColorProfile, ColorProfileRaw, ColorSpace, CompressionFormat, EncoderQuality,
    HeifContext, Image, LibHeif, RgbChroma, color_profile_types,
};
use photodesk_color::colour::{DISPLAY_P3, SRGB, encoded_to_lab};
use photodesk_color::corpus::{self, COLORCHECKER_SRGB};
use photodesk_color::delta_e::{DeltaStats, ciede2000};
use photodesk_color::working::{Pipeline, Precision};

const PATCH: u32 = 8;
const COLS: u32 = 6;
const ROWS: u32 = 4;
const W: u32 = PATCH * COLS;
const H: u32 = PATCH * ROWS;

/// A Display P3 ICC profile, built by lcms2 rather than copied from a file.
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

/// The 24 patches re-encoded as Display P3, laid out as a 6x4 chart.
fn chart_p3() -> (Vec<[u8; 3]>, Vec<u8>) {
    let patches: Vec<[u8; 3]> = corpus::colorchecker_display_p3()
        .iter()
        .map(|p| {
            [
                (p[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                (p[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                (p[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            ]
        })
        .collect();
    let mut rgb = vec![0u8; (W * H * 3) as usize];
    for y in 0..H {
        for x in 0..W {
            let idx = ((y / PATCH) * COLS + (x / PATCH)) as usize;
            let c = patches[idx.min(23)];
            let o = ((y * W + x) * 3) as usize;
            rgb[o..o + 3].copy_from_slice(&c);
        }
    }
    (patches, rgb)
}

/// Which container formats this machine can actually write losslessly.
///
/// `Uncompressed` is preferred and tried first: any codec loss would land in the ΔE
/// and be indistinguishable from a profile error, which is the one thing this test
/// must not confuse.
fn writable_formats() -> Vec<(&'static str, CompressionFormat)> {
    let lh = LibHeif::new();
    [
        ("uncompressed", CompressionFormat::Uncompressed),
        ("AV1 (AVIF)", CompressionFormat::Av1),
        ("HEVC (iPhone HEIC)", CompressionFormat::Hevc),
    ]
    .into_iter()
    .filter(|(_, f)| !lh.encoder_descriptors(4, Some(*f), None).is_empty())
    .collect()
}

#[test]
fn icc_survives_a_real_container_and_selects_the_right_transform() {
    let lh = LibHeif::new();
    let icc = display_p3_icc();
    println!("Display P3 ICC built by lcms2: {} bytes", icc.len());

    let formats = writable_formats();
    assert!(
        !formats.is_empty(),
        "libheif {:?} can encode nothing — the corpus cannot be built here",
        lh.version()
    );
    let (label, format) = formats[0];
    println!("container: {label}");

    let (patches, rgb) = chart_p3();

    // --- build the container ------------------------------------------------
    let mut img = Image::new(W, H, ColorSpace::Rgb(RgbChroma::Rgb)).expect("create image");
    img.create_plane(Channel::Interleaved, W, H, 8).expect("create plane");
    {
        let mut planes = img.planes_mut();
        let plane = planes.interleaved.as_mut().expect("interleaved plane");
        for y in 0..H as usize {
            let dst = y * plane.stride;
            let src = y * (W * 3) as usize;
            plane.data[dst..dst + (W * 3) as usize]
                .copy_from_slice(&rgb[src..src + (W * 3) as usize]);
        }
    }
    img.set_color_profile_raw(&ColorProfileRaw::new(color_profile_types::PROF, icc.clone()))
        .expect("attach ICC");

    let mut ctx = HeifContext::new().expect("context");
    let mut encoder = lh.encoder_for_format(format).expect("encoder");
    let _ = encoder.set_quality(EncoderQuality::LossLess);
    ctx.encode_image(&img, &mut encoder, None).expect("encode");
    let bytes = ctx.write_to_bytes().expect("serialise container");
    println!("encoded {} bytes", bytes.len());

    // --- read it back as an opaque file -------------------------------------
    let ctx = HeifContext::read_from_bytes(&bytes).expect("reopen container");
    let handle = ctx.primary_image_handle().expect("primary handle");
    assert_eq!((handle.width(), handle.height()), (W, H));

    let recovered = handle
        .color_profile_raw()
        .expect("no ICC came back out of the container — this is the failure the test exists for");
    println!("recovered ICC: {} bytes, type {:?}",
             recovered.data.len(), recovered.profile_type());
    assert_eq!(recovered.data, icc, "the ICC that came out is not the one that went in");

    // The profile has to be *usable*, not merely present. Parse it back with lcms2 and
    // check it describes the primaries we wrote — a container can round-trip bytes
    // while the app still fails to act on them.
    let parsed = lcms2::Profile::new_icc(&recovered.data).expect("recovered ICC does not parse");
    let red = match parsed.read_tag(lcms2::TagSignature::RedColorantTag) {
        lcms2::Tag::CIEXYZ(xyz) => *xyz,
        other => panic!("no red colorant in the recovered profile: {other:?}"),
    };
    println!("recovered red colorant XYZ: ({:.4}, {:.4}, {:.4})", red.X, red.Y, red.Z);
    // Display P3's red primary is far enough from sRGB's that the two cannot be
    // confused: sRGB lands near X 0.4361, P3 near X 0.5151.
    assert!(
        red.X > 0.48,
        "the recovered profile's red primary is at X {:.4}, which is sRGB's, not P3's — \
         the container round-tripped the bytes but they describe the wrong space",
        red.X
    );

    // --- decode and drive the pipeline from what the file said ---------------
    let decoded = lh
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
        .expect("decode");
    let planes = decoded.planes();
    let plane = planes.interleaved.expect("interleaved plane");

    let pipeline = Pipeline::new(Precision::F16, 5);
    let mut deltas = Vec::new();
    for (i, want) in patches.iter().enumerate() {
        // Sample the middle of each patch, away from any block boundary.
        let px = (i as u32 % COLS) * PATCH + PATCH / 2;
        let py = (i as u32 / COLS) * PATCH + PATCH / 2;
        let o = py as usize * plane.stride + (px * 3) as usize;
        let got = [plane.data[o], plane.data[o + 1], plane.data[o + 2]];
        assert_eq!(
            got, *want,
            "patch {i} decoded to {got:?}, expected {want:?} — the container is lossy, \
             so any colour difference below would be codec loss rather than profile handling"
        );

        // The point of the whole test: interpret the file through the profile it
        // carries, export to sRGB, and check it matches the sRGB we started from.
        let out = pipeline.round_trip_u8(got, &DISPLAY_P3, &SRGB);
        let expected = COLORCHECKER_SRGB[i];
        let a = encoded_to_lab(
            [out[0] as f32 / 255.0, out[1] as f32 / 255.0, out[2] as f32 / 255.0], &SRGB);
        let b = encoded_to_lab(
            [expected[0] as f32 / 255.0, expected[1] as f32 / 255.0, expected[2] as f32 / 255.0],
            &SRGB);
        deltas.push(ciede2000(a, b));
    }

    let stats = DeltaStats::from(&deltas);
    println!("P3-tagged container -> sRGB export vs the original sRGB patches: {stats}");
    assert!(stats.max < 1.5, "round trip through a tagged container exceeded ΔE 1.5: {stats}");

    // The counterexample. If the profile were ignored and the file read as sRGB, the
    // error has to be large enough that the assertion above would have caught it —
    // otherwise this test cannot distinguish reading the tag from ignoring it.
    let ignored: Vec<f64> = patches
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let out = pipeline.round_trip_u8(*p, &SRGB, &SRGB); // wrong: assume sRGB
            let expected = COLORCHECKER_SRGB[i];
            let a = encoded_to_lab(
                [out[0] as f32 / 255.0, out[1] as f32 / 255.0, out[2] as f32 / 255.0], &SRGB);
            let b = encoded_to_lab(
                [expected[0] as f32 / 255.0, expected[1] as f32 / 255.0, expected[2] as f32 / 255.0],
                &SRGB);
            ciede2000(a, b)
        })
        .collect();
    let ignored_stats = DeltaStats::from(&ignored);
    println!("same file with the profile ignored (RapidRAW's behaviour): {ignored_stats}");
    assert!(
        ignored_stats.max > 1.5,
        "ignoring the profile costs only {ignored_stats}, which would pass the assertion \
         above — so this test cannot tell reading the tag from ignoring it"
    );
}
