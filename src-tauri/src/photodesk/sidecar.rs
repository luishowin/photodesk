//! The sidecar on disk: where it lives, how it is written, and what must never happen
//! to the photograph next to it.
//!
//! §0's first frozen item is "the source file is never modified", and this module is
//! the one place in the document path that touches a filesystem at all — so it is
//! where that invariant is either kept or lost. It is kept structurally: nothing here
//! opens the source for anything but reading, and [`hash_source`] is the only function
//! that opens it at all.
//!
//! **Writes are atomic.** A sidecar is the user's edits; a half-written one is worse
//! than no sidecar at all, because it looks like a corrupt document rather than an
//! absent one, and §6.3 has no row for "truncated". So a save writes a temporary file
//! beside the target and renames it over, which POSIX makes atomic within a directory
//! — beside it rather than in `/tmp` precisely because a rename across filesystems is
//! not a rename, it is a copy that can fail halfway.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::document::{Document, DocumentError, Loaded};

/// `IMG_4821.HEIC` → `IMG_4821.HEIC.photodesk.json`.
///
/// **This is not quite what §6.1 shows, and the difference is deliberate.** §6.1's
/// example drops the extension — `IMG_4821.HEIC` beside `IMG_4821.photodesk.json` —
/// which is tidier and which collides in the workflow this application exists for.
/// §14 gives v0.1 "open a HEIF → … → export", and an export that lands beside its
/// source is `IMG_4821.jpg` next to `IMG_4821.HEIC`. Under §6.1's naming those two
/// photographs share one sidecar, so editing the export silently overwrites the
/// original's edits.
///
/// Keeping the whole filename removes the collision rather than detecting it, at the
/// cost of a longer name that still sorts beside its photograph. darktable resolved
/// the same question the same way, for the same reason. Recorded in `DECISIONS.md` as
/// a correction to §6.1.
pub fn sidecar_path(source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    source.with_file_name(format!("{name}.photodesk.json"))
}

/// The cache directory for a photograph's folder (§6.1). Disposable, and never named
/// in a document — see `document::validate`.
pub fn cache_dir(source: &Path) -> PathBuf {
    source
        .parent()
        .unwrap_or(Path::new("."))
        .join(".photodesk")
}

/// `blake3:<hex>` of a source file.
///
/// Opened read-only and streamed, so a 50 MB RAW does not become 50 MB of resident
/// memory to answer a question about identity.
pub fn hash_source(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}

/// Errors from the sidecar itself, as distinct from the document inside it.
#[derive(Debug)]
pub enum SidecarError {
    Io(std::io::Error),
    Document(DocumentError),
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidecarError::Io(e) => write!(f, "{e}"),
            SidecarError::Document(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SidecarError {}

impl From<std::io::Error> for SidecarError {
    fn from(e: std::io::Error) -> Self {
        SidecarError::Io(e)
    }
}

impl From<DocumentError> for SidecarError {
    fn from(e: DocumentError) -> Self {
        SidecarError::Document(e)
    }
}

/// Read a sidecar, applying §6.3 in full. See [`Loaded`].
pub fn load(path: &Path) -> Result<Loaded, SidecarError> {
    let text = std::fs::read_to_string(path)?;
    Ok(super::document::from_json(&text)?)
}

/// Write a document to `path`, atomically.
///
/// Takes the document **by value-borrow rather than through a [`Loaded`]**, because
/// the only way to obtain an owned `Document` from a load is `into_writable`, which
/// is where §6.3's read-only rule is enforced. Saving therefore cannot be reached for
/// a read-only document without going past that check first.
pub fn save(path: &Path, document: &Document) -> Result<(), SidecarError> {
    let text = to_canonical_json(document).map_err(|e| {
        SidecarError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;

    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;

    // Same directory, so the rename below is a rename rather than a cross-filesystem
    // copy. The pid keeps two processes editing the same photo from colliding.
    let temp = path.with_extension(format!("json.tmp{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        // Durable before it is visible: the rename is atomic with respect to other
        // readers, but not with respect to a power cut, and the cheap half of that
        // problem is worth solving.
        file.sync_all()?;
    }
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            Err(SidecarError::Io(e))
        }
    }
}

/// The canonical serialisation: two-space indent, a trailing newline, fields in
/// declaration order.
///
/// Canonical because §0 makes the document human-readable and therefore diffable, and
/// a file whose formatting depends on which code path wrote it produces diffs about
/// nothing. It is also what makes "opening a photograph does not modify its sidecar"
/// a byte-level claim rather than a semantic one.
pub fn to_canonical_json(document: &Document) -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(document)?;
    text.push('\n');
    Ok(text)
}
