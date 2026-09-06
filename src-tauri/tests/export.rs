//! Export, and §12.3 with all five of its steps.
//!
//! > ```text
//! > for each source in corpus:
//! >     hash_before = blake3(file)
//! >     open → apply document → render preview → export → close
//! >     assert blake3(file) == hash_before
//! > ```
//! > Byte-identical. Not "metadata unchanged" — identical. **This is invariant #1 and
//! > it's the cheapest possible test for the most expensive possible bug.**
//!
//! `sidecar.rs` has been asserting the writing half of that since the document model
//! landed. This is the first time the whole chain exists to run.
//!
//! The GPU tests skip when there is no adapter; the encoder and metadata ones need no
//! GPU at all and always run.

use half::f16;
use photodesk::engine::colour::{DISPLAY_P3, SRGB};
use photodesk::engine::export::{self, ExportError, SourceMetadata};
use photodesk::engine::image::Image;
use photodesk::engine::render::{Rendered, Renderer};
use photodesk::engine::{decode, exif, icc};
use photodesk::photodesk::document::{
    AdjustV1, ColorSpace, Document, Layer, MetadataPolicy, Op, Output, OutputFormat, Params, Source,
};
use photodesk::photodesk::{graph, sidecar};

macro_rules! renderer {
    () => {
        match Renderer::new() {
            Ok(r) => r,
            Err(e) => {
                println!("SKIP: {e}");
                return;
            }
        }
    };
}

/// A JPEG with EXIF, generated once with Pillow and inlined — this crate has decoders
/// and no encoder for building fixtures with metadata already in them.
///
/// 8×8, orientation 6 (rotate 90° clockwise on display), a GPS block, and a camera
/// make and model. Everything §6.1's `metadata` policy has an opinion about.
const EXIF_JPEG: &[u8] = include_bytes!("fixtures/exif.jpg");

fn output(format: OutputFormat, metadata: MetadataPolicy) -> Output {
    Output {
        format,
        quality: Some(92),
        colorspace: ColorSpace::Srgb,
        metadata,
    }
}

fn render_something(r: &Renderer) -> Rendered {
    let image = Image::new(8, 8, vec![f16::from_f32(0.25); 8 * 8 * 3]);
    let doc = Document::new(Source {
        file: "a.jpg".into(),
        hash: "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        dimensions: [8, 8],
        colorspace: ColorSpace::DisplayP3,
        orientation: 1,
    });
    r.render(&graph::compile(&doc).expect("compile"), &image)
        .expect("render")
}

// ------------------------------------------------------------------ the profile

/// §4's chain ends "→ ICC-tagged file", and this is the tag. Written in-tree for the
/// same reason it is read in-tree; the harness checks lcms2 can read it.
#[test]
fn the_profile_we_write_is_the_profile_we_read() {
    for space in [&SRGB, &DISPLAY_P3] {
        let bytes = icc::write(space);
        println!("{}: {} bytes", space.name, bytes.len());

        let back = icc::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", space.name));
        let classified = back.classify().unwrap_or_else(|e| panic!("{}: {e}", space.name));
        assert_eq!(classified.name, space.name);

        // The colorants survive the D65 → D50 → D65 round trip, which is the step this
        // module and the parser each do once in opposite directions.
        let want = space.to_xyz();
        let worst = (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| (back.to_xyz_d65.0[i][j] - want.0[i][j]).abs())
            .fold(0.0, f64::max);
        println!("  worst colorant difference after a write/read round trip: {worst:.2e}");
        assert!(worst < 1e-4, "{}: {worst:.3e}", space.name);
    }

    // Real profiles are small. Apple's Display P3 is 536 bytes and this shares one
    // curve tag across three channels for the same reason.
    assert!(icc::write(&DISPLAY_P3).len() < 1024);
}

/// Exporting the same document twice produces the same bytes.
///
/// §12.1's golden images cannot be blessed otherwise: a reference that differs from the
/// render by the second it was made in fails every time it is run.
#[test]
fn export_is_deterministic() {
    let r = renderer!();
    let rendered = render_something(&r);
    let source = SourceMetadata::from_jpeg(EXIF_JPEG);

    for format in [OutputFormat::Jpeg, OutputFormat::Png] {
        let out = output(format, MetadataPolicy::KeepMinusGps);
        let a = export::encode(&rendered, &out, &source).expect("encode");
        let b = export::encode(&rendered, &out, &source).expect("encode");
        println!("{format:?}: {} bytes, identical on a second run: {}", a.len(), a == b);
        assert_eq!(a, b, "{format:?} export is not deterministic");
    }
}

/// An exported file has a profile on it, and the profile says what the document asked
/// for.
#[test]
fn an_exported_file_carries_its_colour_space() {
    let r = renderer!();
    let rendered = render_something(&r);
    let empty = SourceMetadata::default();

    let jpeg = export::encode(&rendered, &output(OutputFormat::Jpeg, MetadataPolicy::Strip), &empty)
        .expect("encode");
    // Read it back with the product's own decoder, which is the thing that will have
    // to open this file if it is ever re-imported.
    let back = decode::decode(&jpeg).expect("our own export must open in our own decoder");
    println!("re-imported: {:?} {}", back.tag, back.source_space.name);
    assert_eq!(back.source_space.name, SRGB.name);
    assert!(matches!(back.tag, decode::ColourTag::Icc { .. }));

    let png = export::encode(&rendered, &output(OutputFormat::Png, MetadataPolicy::Strip), &empty)
        .expect("encode");
    // `iCCP` before `IDAT`, or a reader is entitled to have stopped looking.
    let iccp = png.windows(4).position(|w| w == b"iCCP").expect("no iCCP chunk");
    let idat = png.windows(4).position(|w| w == b"IDAT").expect("no IDAT chunk");
    println!("png: {} bytes, iCCP at {iccp}, IDAT at {idat}", png.len());
    assert!(iccp < idat, "the profile is written after the pixels");
}

/// A format the document may name and this build cannot write is refused by name, not
/// substituted.
#[test]
fn an_unwritable_format_is_refused_rather_than_substituted() {
    let r = renderer!();
    let rendered = render_something(&r);
    let err = export::encode(
        &rendered,
        &output(OutputFormat::Tiff, MetadataPolicy::Strip),
        &SourceMetadata::default(),
    )
    .expect_err("TIFF is not written");
    println!("{err}");
    assert!(matches!(err, ExportError::Unsupported { .. }));
}

// -------------------------------------------------------------- §6.1's metadata

/// The fixture says what the tests below are about, so a change to it is visible.
#[test]
fn the_fixture_has_everything_the_policy_has_an_opinion_about() {
    let e = exif::from_jpeg(EXIF_JPEG).expect("the fixture carries EXIF");
    println!(
        "orientation {}, GPS present {}, IFD0 tags {:04x?}, Exif IFD tags {:04x?}",
        e.orientation(),
        e.has_gps(),
        e.ifd0_tags(),
        e.exif_tags()
    );
    assert_eq!(e.orientation(), 6, "the fixture is supposed to be rotated");
    assert!(e.has_gps(), "the fixture is supposed to carry a location");
    assert!(e.ifd0_tags().len() > 2, "the fixture has too little metadata to be about anything");
}

/// §6.1's default. **GPS is removed, not unreferenced** — deleting the pointer while
/// leaving the bytes is not what a privacy default means.
#[test]
fn keep_minus_gps_removes_the_location_from_the_bytes() {
    let e = exif::from_jpeg(EXIF_JPEG).expect("EXIF");
    let kept = e.to_bytes(true);
    let trimmed = e.to_bytes(false);
    println!("keep {} bytes, keep-minus-gps {} bytes", kept.len(), trimmed.len());

    let with_gps = exif::parse(&kept).expect("re-parse");
    let without = exif::parse(&trimmed).expect("re-parse");
    assert!(with_gps.has_gps(), "`keep` dropped the GPS block");
    assert!(!without.has_gps(), "`keep-minus-gps` kept the GPS block");

    // The camera survives — the policy is about location, not about erasing the
    // photograph's provenance.
    assert_eq!(
        with_gps.ifd0_tags().iter().filter(|t| **t != 0x8825).count(),
        without.ifd0_tags().len(),
        "removing GPS took something else with it"
    );

    // And the trimmed block is genuinely smaller: the coordinates are gone from the
    // file rather than merely unreachable through the tag tree.
    assert!(
        trimmed.len() < kept.len(),
        "the GPS bytes are still in the block, just unreferenced — which is exactly \
         the thing this policy is not allowed to do"
    );
}

/// A stale thumbnail of the *source* defeats a crop, and confuses a file browser even
/// when it does not. Dropped under every policy.
#[test]
fn the_source_thumbnail_never_survives() {
    let e = exif::from_jpeg(EXIF_JPEG).expect("EXIF");
    for keep_gps in [true, false] {
        let bytes = e.to_bytes(keep_gps);
        // IFD1 is reached through IFD0's "next IFD" pointer, which is the last four
        // bytes of IFD0. Zero means there is no second directory.
        let big = &bytes[0..2] == b"MM";
        let count = if big {
            u16::from_be_bytes([bytes[8], bytes[9]])
        } else {
            u16::from_le_bytes([bytes[8], bytes[9]])
        } as usize;
        let next_at = 10 + count * 12;
        let next = if big {
            u32::from_be_bytes([bytes[next_at], bytes[next_at + 1], bytes[next_at + 2], bytes[next_at + 3]])
        } else {
            u32::from_le_bytes([bytes[next_at], bytes[next_at + 1], bytes[next_at + 2], bytes[next_at + 3]])
        };
        println!("keep_gps={keep_gps}: IFD0 has {count} entries, next IFD offset {next}");
        assert_eq!(next, 0, "an IFD1 survived, and with it a thumbnail of the source");
    }
}

/// `strip` means strip.
#[test]
fn strip_writes_no_metadata_at_all() {
    let r = renderer!();
    let rendered = render_something(&r);
    let source = SourceMetadata::from_jpeg(EXIF_JPEG);

    let stripped =
        export::encode(&rendered, &output(OutputFormat::Jpeg, MetadataPolicy::Strip), &source)
            .expect("encode");
    assert!(exif::from_jpeg(&stripped).is_none(), "`strip` left an EXIF block behind");

    let kept =
        export::encode(&rendered, &output(OutputFormat::Jpeg, MetadataPolicy::Keep), &source)
            .expect("encode");
    let back = exif::from_jpeg(&kept).expect("`keep` dropped the EXIF block");
    println!("kept: orientation {}, GPS {}", back.orientation(), back.has_gps());
    assert!(back.has_gps());
}

/// **Orientation is written as 1, because the pixels are already upright.**
///
/// This is why `strip` can be honest. EXIF orientation is structure rather than
/// description: a file whose pixels are sideways and whose tag says "rotate me" reads
/// correctly only to software that honours the tag, so stripping the metadata would
/// rotate the photograph. The decoder turns the pixels instead, and the exported tag
/// has nothing left to say.
#[test]
fn a_rotated_source_exports_upright_with_a_neutral_tag() {
    // The fixture is 8×8 so a rotation is not visible in the dimensions; what is
    // checkable is that the tag the decoder saw was 6 and the tag we write is 1.
    let decoded = decode::decode(EXIF_JPEG).expect("decode");
    println!("decoded orientation {} (applied)", decoded.orientation);
    assert_eq!(decoded.orientation, 6, "the decoder did not see the fixture's rotation");

    let e = exif::from_jpeg(EXIF_JPEG).expect("EXIF");
    let written = exif::parse(&e.to_bytes(true)).expect("re-parse");
    assert_eq!(
        written.orientation(),
        1,
        "the export carried the source's orientation forward, so anything honouring \
         the tag will rotate a picture that is already upright"
    );
}

/// The decoder really does turn the pixels, and turns them the right way.
#[test]
fn orientation_is_applied_to_the_pixels() {
    // A 4×2 image whose top-left pixel is the only bright one, so where it ends up
    // says which transform ran.
    let mut pixels = vec![f16::ZERO; 4 * 2 * 3];
    for c in 0..3 {
        pixels[c] = f16::ONE;
    }
    let image = Image::new(4, 2, pixels);

    // Orientation 6 is "rotate 90° clockwise for display": a 4×2 becomes 2×4 and the
    // top-left corner lands at the top-right.
    let turned = image.oriented(6);
    assert_eq!((turned.width(), turned.height()), (2, 4));
    assert_eq!(turned.pixel(1, 0), [1.0, 1.0, 1.0], "the corner is not where a 90° CW turn puts it");
    assert_eq!(turned.pixel(0, 0), [0.0, 0.0, 0.0]);

    // Every orientation is a permutation: the same pixels, rearranged. A transform
    // that dropped or duplicated one would still "work" and would be wrong.
    for orientation in 1..=8u8 {
        let out = image.oriented(orientation);
        assert_eq!(
            out.width() as usize * out.height() as usize,
            8,
            "orientation {orientation} changed the pixel count"
        );
        let bright = (0..out.height())
            .flat_map(|y| (0..out.width()).map(move |x| (x, y)))
            .filter(|(x, y)| out.pixel(*x, *y)[0] > 0.5)
            .count();
        assert_eq!(bright, 1, "orientation {orientation} lost or duplicated the marked pixel");
    }
    // 1 is the identity, and an out-of-range value is treated as one rather than
    // panicking on a file somebody else wrote.
    assert_eq!(image.oriented(1), image);
    assert_eq!(image.oriented(9), image);
}

// ------------------------------------------------------------------------ §12.3

/// **§12.3, with all five steps for the first time.**
///
/// > hash_before = blake3(file); open → apply document → render preview → export →
/// > close; assert blake3(file) == hash_before.
///
/// Byte-identical, not "metadata unchanged". The mtime is checked too, because a
/// rewrite with identical bytes is still a rewrite and it is the kind that survives a
/// hash comparison.
#[test]
fn section_12_3_the_source_file_is_never_modified() {
    let r = renderer!();
    let dir = std::env::temp_dir().join(format!("photodesk-12-3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let photo = dir.join("IMG_4821.jpg");
    std::fs::write(&photo, EXIF_JPEG).expect("write the source");
    let before = sidecar::hash_source(&photo).expect("hash");
    let mtime_before = std::fs::metadata(&photo).unwrap().modified().unwrap();

    // ---- open ----
    let decoded = decode::open(&photo).expect("open");
    let source_bytes = std::fs::read(&photo).expect("read for metadata");
    let metadata = SourceMetadata::from_jpeg(&source_bytes);

    // ---- apply a document ----
    let document = Document {
        stack: vec![Layer {
            id: "global".into(),
            op: Op::Adjust,
            op_version: 1,
            enabled: true,
            name: None,
            opacity: None,
            mask: None,
            params: Params::AdjustV1(AdjustV1 {
                exposure: Some(0.35),
                contrast: Some(-4.0),
                highlights: Some(-22.0),
                shadows: Some(18.0),
                temperature: Some(-300.0),
                ..Default::default()
            }),
        }],
        ..Document::new(Source {
            file: "IMG_4821.jpg".into(),
            hash: before.clone(),
            dimensions: [decoded.image.width(), decoded.image.height()],
            colorspace: ColorSpace::Srgb,
            orientation: decoded.orientation,
        })
    };
    let sidecar_path = sidecar::sidecar_path(&photo);
    sidecar::save(&sidecar_path, &document).expect("save the sidecar");

    // ---- render preview ----
    let compiled = graph::compile(&document).expect("compile");
    let proxy_edge = Image::proxy_longest_edge(
        decoded.image.width().max(decoded.image.height()),
        1920,
    );
    let preview = r
        .render(&compiled, &decoded.image.proxy(proxy_edge))
        .expect("preview render");

    // ---- export ----
    let full = r.render(&compiled, &decoded.image).expect("export render");
    let file = export::encode(&full, &document.output, &metadata).expect("encode");
    let exported = dir.join("IMG_4821-edited.jpg");
    std::fs::write(&exported, &file).expect("write the export");

    // ---- close, and the assertion ----
    let after = sidecar::hash_source(&photo).expect("re-hash");
    println!(
        "source {} bytes, preview {:?}, export {} bytes\n  hash before {}\n  hash after  {}",
        source_bytes.len(),
        preview,
        file.len(),
        &before[..24],
        &after[..24]
    );
    assert_eq!(
        before, after,
        "the source file changed. §0's first frozen item is that it never does, and \
         §12.3 calls this the cheapest possible test for the most expensive possible bug"
    );
    assert_eq!(
        mtime_before,
        std::fs::metadata(&photo).unwrap().modified().unwrap(),
        "the source was rewritten with identical bytes, which is still a rewrite"
    );

    // The counterexample, so a hash that cannot see a change is not mistaken for a
    // test that passed.
    std::fs::write(&photo, b"different").unwrap();
    assert_ne!(before, sidecar::hash_source(&photo).unwrap());

    // And the export is a real file our own decoder opens, with the location gone.
    let reopened = decode::decode(&file).expect("the export must open");
    assert_eq!(reopened.source_space.name, SRGB.name);
    let written_exif = exif::from_jpeg(&file).expect("keep-minus-gps writes EXIF");
    assert!(!written_exif.has_gps(), "the export carried the location out with it");
    assert_eq!(written_exif.orientation(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}
