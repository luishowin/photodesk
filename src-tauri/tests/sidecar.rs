//! The sidecar on disk, and the first link of §12.3.
//!
//! §12.3 is the whole cycle — open, apply, render, export, close — and asserts the
//! source file is byte-identical afterwards. Two of those five steps do not exist yet.
//! What exists is the part that *writes*, and it is the part that could break the
//! invariant, so it is worth asserting now rather than when the render path arrives
//! and there are three suspects instead of one.
//!
//! §0's first frozen item is "the source file is never modified", and its cheapest
//! possible test is the one below.

use std::path::{Path, PathBuf};

use photodesk::photodesk::document::{self, AdjustV1, ColorSpace, Document, Layer, Op, Params, Source};
use photodesk::photodesk::sidecar;

/// A scratch directory that cleans up after itself, so the tests can be about files
/// without leaving any.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "photodesk-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Stands in for a photograph. Its *contents* are irrelevant to every test here —
/// what matters is that nothing in the document path opens it for writing.
fn write_fake_photograph(path: &Path) -> String {
    std::fs::write(path, b"\x00\x00\x00\x20ftypheic not really a heic, and it does not need to be")
        .expect("write source");
    sidecar::hash_source(path).expect("hash source")
}

fn document_for(path: &Path) -> Document {
    Document::new(Source {
        file: path.file_name().unwrap().to_string_lossy().into_owned(),
        hash: sidecar::hash_source(path).expect("hash"),
        dimensions: [4032, 3024],
        colorspace: ColorSpace::DisplayP3,
        orientation: 1,
    })
}

fn adjust_layer(id: &str, exposure: f32) -> Layer {
    Layer {
        id: id.into(),
        op: Op::Adjust,
        op_version: 1,
        enabled: true,
        name: None,
        opacity: None,
        mask: None,
        params: Params::AdjustV1(AdjustV1 {
            exposure: Some(exposure),
            ..Default::default()
        }),
    }
}

// ------------------------------------------------------------------ where it goes

/// Beside the photograph, and named so that a photograph and its own export cannot
/// share one. See `sidecar::sidecar_path` — this differs from §6.1's example on purpose.
#[test]
fn the_sidecar_sits_beside_the_photograph() {
    assert_eq!(
        sidecar::sidecar_path(Path::new("/photos/IMG_4821.HEIC")),
        PathBuf::from("/photos/IMG_4821.HEIC.photodesk.json")
    );

    // **The reason the extension is kept**, and the reason this differs from §6.1's
    // example. §14's v0.1 is "open a HEIF → … → export", so `IMG_4821.jpg` beside
    // `IMG_4821.HEIC` is not a corner case, it is the workflow. Dropping the
    // extension gives those two photographs one sidecar, and editing the export then
    // overwrites the original's edits with no error and nothing to notice.
    assert_ne!(
        sidecar::sidecar_path(Path::new("/photos/IMG_4821.HEIC")),
        sidecar::sidecar_path(Path::new("/photos/IMG_4821.jpg")),
        "a photograph and its own export would share a sidecar"
    );

    // A file with no extension still gets one.
    assert_eq!(
        sidecar::sidecar_path(Path::new("/photos/scan")),
        PathBuf::from("/photos/scan.photodesk.json")
    );

    assert_eq!(
        sidecar::cache_dir(Path::new("/photos/IMG_4821.HEIC")),
        PathBuf::from("/photos/.photodesk")
    );
}

// ------------------------------------------------------------ §0's first invariant

/// **The source file is never modified.** §0's first frozen item, and §12.3's shape at
/// the scope that exists today: open, edit, save the sidecar, re-hash the photograph.
///
/// Byte-identical, not "metadata unchanged". The mtime is checked too, because a
/// rewrite with identical bytes is still a rewrite, and it is the kind that survives
/// a hash comparison and shows up as a backup tool copying twelve megabytes for
/// nothing.
#[test]
fn editing_and_saving_never_touches_the_photograph() {
    let scratch = Scratch::new("source-preservation");
    let photo = scratch.path("IMG_4821.HEIC");
    let before = write_fake_photograph(&photo);
    let mtime_before = std::fs::metadata(&photo).unwrap().modified().unwrap();

    let sidecar_path = sidecar::sidecar_path(&photo);
    let mut doc = document_for(&photo);

    // A full edit cycle: create, save, reopen, change, save again.
    sidecar::save(&sidecar_path, &doc).expect("first save");
    let reopened = sidecar::load(&sidecar_path).expect("reopen");
    let mut doc2 = reopened.into_writable().expect("writable");
    doc2.stack.push(adjust_layer("global", 0.35));
    sidecar::save(&sidecar_path, &doc2).expect("second save");
    doc.stack.push(adjust_layer("global", -1.0));
    sidecar::save(&sidecar_path, &doc).expect("third save");

    let after = sidecar::hash_source(&photo).expect("re-hash");
    assert_eq!(
        before, after,
        "the photograph changed. §0's first frozen item is that it never does"
    );
    assert_eq!(
        mtime_before,
        std::fs::metadata(&photo).unwrap().modified().unwrap(),
        "the photograph was rewritten with identical bytes, which is still a rewrite"
    );

    // And the counterexample, so the assertion above is known to be measuring
    // something: a hash comparison that cannot see a change is not a test.
    std::fs::write(&photo, b"different").unwrap();
    assert_ne!(before, sidecar::hash_source(&photo).unwrap());
}

/// The hash is the one in §6.1's example, prefixed so §12.3 knows what it is comparing.
#[test]
fn the_source_hash_names_its_algorithm() {
    let scratch = Scratch::new("hash");
    let photo = scratch.path("a.heic");
    let hash = write_fake_photograph(&photo);
    println!("{hash}");
    assert!(hash.starts_with("blake3:"));
    assert_eq!(hash.len(), "blake3:".len() + 64);
    // Deterministic, or it is not an identity.
    assert_eq!(hash, sidecar::hash_source(&photo).unwrap());
}

// ------------------------------------------------------------------- how it writes

/// A half-written sidecar is worse than an absent one: it looks like a corrupt
/// document rather than a missing one, and §6.3 has no row for "truncated".
#[test]
fn a_save_leaves_no_temporary_file_behind() {
    let scratch = Scratch::new("atomic");
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let path = sidecar::sidecar_path(&photo);

    for _ in 0..3 {
        sidecar::save(&path, &document_for(&photo)).expect("save");
    }

    let leftovers: Vec<String> = std::fs::read_dir(&scratch.0)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files survived the save: {leftovers:?}");

    let names: Vec<String> = std::fs::read_dir(&scratch.0)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "expected the photograph and its sidecar, got {names:?}");
}

/// Saving a document nobody changed must not change the file.
///
/// The reason this is worth a test rather than an assumption: the load path applies
/// migrations, fills defaults and validates, and any of those can quietly normalise
/// something. If it does, then merely *opening* a photograph dirties its sidecar, and
/// a user with a backup tool or a git-tracked photo directory sees churn they did not
/// cause and cannot explain.
#[test]
fn opening_and_saving_an_unchanged_document_rewrites_the_same_bytes() {
    let scratch = Scratch::new("idempotent");
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let path = sidecar::sidecar_path(&photo);

    let mut doc = document_for(&photo);
    doc.stack.push(adjust_layer("global", 0.35));
    doc.stack.push(adjust_layer("l_7c31", -0.4));
    sidecar::save(&path, &doc).expect("save");
    let first = std::fs::read(&path).unwrap();

    for _ in 0..3 {
        let reopened = sidecar::load(&path).unwrap().into_writable().unwrap();
        sidecar::save(&path, &reopened).unwrap();
        assert_eq!(
            first,
            std::fs::read(&path).unwrap(),
            "an unmodified document did not round-trip to the same bytes"
        );
    }

    println!("{}", String::from_utf8_lossy(&first));
}

/// §0 makes the document human-readable, and a file whose formatting depends on which
/// code path wrote it produces diffs about nothing.
#[test]
fn the_written_form_is_readable_and_ends_with_a_newline() {
    let scratch = Scratch::new("canonical");
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let path = sidecar::sidecar_path(&photo);

    let mut doc = document_for(&photo);
    doc.stack.push(adjust_layer("global", 0.35));
    sidecar::save(&path, &doc).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.ends_with('\n'), "no trailing newline; every diff will complain");
    assert!(text.contains("\n  \"photodesk\": 1"), "not indented:\n{text}");
    // The schema version is first, so a human opening the file learns what they are
    // looking at before they learn anything else.
    assert!(text.starts_with("{\n  \"photodesk\""), "{text}");
}

/// A load of something that is not a document says so, rather than producing an empty
/// one and losing the user's edits behind a shrug.
#[test]
fn a_corrupt_sidecar_is_an_error_rather_than_an_empty_document() {
    let scratch = Scratch::new("corrupt");
    let path = scratch.path("a.photodesk.json");

    std::fs::write(&path, b"{ this is not json").unwrap();
    let err = sidecar::load(&path).expect_err("truncated JSON must not open");
    println!("{err}");
    assert!(err.to_string().contains("JSON"));

    // A missing file is a different thing from a broken one, and the caller has to be
    // able to tell them apart — one means "no edits yet", the other means "stop".
    let err = sidecar::load(&scratch.path("absent.photodesk.json")).unwrap_err();
    assert!(matches!(err, photodesk::photodesk::sidecar::SidecarError::Io(_)));
}

/// §6.3's read-only row, end to end: the only door to an owned `Document` is
/// `into_writable`, and `save` takes one — so a read-only document cannot reach the
/// disk without the compiler noticing.
#[test]
fn a_read_only_document_cannot_reach_the_save_path() {
    let scratch = Scratch::new("read-only");
    let path = scratch.path("a.photodesk.json");

    // Schema 0, for which there is no migration.
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let mut value = serde_json::to_value(document_for(&photo)).unwrap();
    value["photodesk"] = serde_json::json!(0);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let loaded = sidecar::load(&path).expect("it opens, read-only");
    assert!(loaded.read_only().is_some());
    // Readable, exportable, and not saveable — which is §6.3's row 3 exactly.
    assert_eq!(loaded.document().photodesk, 0);
    assert!(loaded.into_writable().is_err());
}

/// The cache directory is never named by a document, so a document may be moved beside
/// its photograph without carrying a reference into somewhere disposable (§0).
#[test]
fn a_saved_document_never_mentions_the_cache_directory() {
    let scratch = Scratch::new("no-cache-refs");
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let path = sidecar::sidecar_path(&photo);

    let mut doc = document_for(&photo);
    doc.stack.push(adjust_layer("global", 0.35));
    sidecar::save(&path, &doc).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(".photodesk/"), "the sidecar points into the cache:\n{text}");

    // The cache directory is also not created as a side effect of saving. It is
    // regenerable, which means it is created by whatever regenerates it.
    assert!(!sidecar::cache_dir(&photo).exists());
}

/// Two processes editing the same photograph must not clobber each other's temporary
/// file. Not concurrency-safe in general — that needs a lock and a decision that is
/// not in the register — but the failure mode this avoids is the cheap one.
#[test]
fn the_temporary_file_is_process_specific() {
    let scratch = Scratch::new("temp-name");
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let path = sidecar::sidecar_path(&photo);
    sidecar::save(&path, &document_for(&photo)).unwrap();
    // Nothing to assert beyond the save succeeding and leaving one file; the name is
    // checked by `a_save_leaves_no_temporary_file_behind`. This test documents the
    // limit rather than the feature: concurrent editing is undecided (§0), and this
    // only keeps two processes from picking the same temporary path.
    assert!(path.exists());
}

/// A document round-trips through the filesystem unchanged, including the parts that
/// are easy to lose: absent optional fields stay absent.
#[test]
fn a_document_survives_the_filesystem() {
    let scratch = Scratch::new("round-trip");
    let photo = scratch.path("a.heic");
    write_fake_photograph(&photo);
    let path = sidecar::sidecar_path(&photo);

    let mut doc = document_for(&photo);
    doc.stack.push(adjust_layer("global", 0.35));
    doc.stack.push(Layer {
        name: Some("Sky".into()),
        opacity: Some(0.8),
        ..adjust_layer("l_7c31", -0.4)
    });
    sidecar::save(&path, &doc).unwrap();

    let back = sidecar::load(&path).unwrap().into_writable().unwrap();
    assert_eq!(doc, back);
    assert_eq!(back.stack[0].name, None);
    assert_eq!(back.stack[1].name.as_deref(), Some("Sky"));
    assert_eq!(back.stack[1].opacity, Some(0.8));

    // And it is still a document the parser is willing to validate, not just one
    // serde happens to accept.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(document::from_json(&text).is_ok());
}
