//! Reading a document, and §6.3's table expressed as types rather than as comments.
//!
//! §6.3 gives six conditions six *different* behaviours, and three of them are not
//! "accept" or "reject" — they are accept-with-a-consequence: open read-only, warn
//! that appearance may differ, write back on next save. A loader that returns a bare
//! `Result<Document, _>` throws all three away and leaves them to be remembered at
//! each call site, which is how "never silently re-render" becomes a re-render.
//!
//! So loading returns a [`Loaded`], and the consequences are in the type:
//!
//! - The document is **not a public field**. [`Loaded::document`] lends it for reading;
//!   [`Loaded::into_writable`] is the only way to get an owned one, and it fails with
//!   the reason when §6.3 said read-only. Since saving takes an owned `Document`,
//!   read-only is enforced by the compiler rather than by everyone's care.
//! - Everything the user has to be told is in [`Loaded::notices`], which is a `Vec`
//!   the caller has to look at rather than a flag it can forget.

use serde_json::Value;

use super::migrate::{self, MIGRATIONS, MigrateError, Migrated, Migration};
use super::schema::{Document, PIPELINE_VERSION};

/// Why a document could not be opened at all.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentError {
    /// Not JSON.
    Syntax(String),
    /// JSON, but not this schema: an unknown key, a missing field, an unknown `op`, an
    /// `op_version` with no schema, a `params` object that does not match. §6.1 and
    /// §6.3 make all of these the same outcome — reject — because a
    /// partially-understood edit is worse than a refused one.
    Schema(String),
    /// §6.3's first row: newer than this build.
    Migrate(MigrateError),
    /// The document parses and is internally inconsistent — see [`super::validate`].
    Invalid(String),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentError::Syntax(e) => write!(f, "not valid JSON: {e}"),
            DocumentError::Schema(e) => write!(f, "not a PhotoDesk document: {e}"),
            DocumentError::Migrate(e) => write!(f, "{e}"),
            DocumentError::Invalid(e) => write!(f, "document is inconsistent: {e}"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Something the user has to be told about a document that opened anyway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// §6.3: "Migrate on load, write back on next save." The write-back is deferred,
    /// so the user's file is not modified by the act of looking at it.
    Migrated { from: u32, to: u32, steps: u32 },
    /// §6.3: "`pipeline_version` older → Open, warn that appearance may differ, offer
    /// explicit re-render on current pipeline. **Never silently re-render.**"
    ///
    /// Carried as a notice precisely because the offer is the user's to accept. The
    /// document keeps its own `pipeline_version` until they do.
    PipelineIsOlder { document: u32, current: u32 },
}

impl std::fmt::Display for Notice {
    /// §6.3 says "explain" and "warn", which are user-facing verbs, so the sentences
    /// live next to the rules that produce them rather than in whatever renders them.
    /// A front end that has to compose this text from an enum tag is a second place
    /// where §6.3's behaviour is described.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Notice::Migrated { from, to, steps } => write!(
                f,
                "this document was written by schema version {from} and has been \
                 migrated to {to} in {steps} step{}. Your file is unchanged until you \
                 save.",
                if *steps == 1 { "" } else { "s" }
            ),
            Notice::PipelineIsOlder { document, current } => write!(
                f,
                "this document was rendered under pipeline version {document} and this \
                 build is on {current}. It is being shown exactly as it was saved — \
                 re-rendering it on the current pipeline may change how it looks, so \
                 that is yours to ask for rather than mine to do quietly."
            ),
        }
    }
}

/// Why a document opened read-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    /// §6.3: "`photodesk` older, no migration → Open **read-only**, explain, offer
    /// export-as-new."
    NoMigrationPath { found: u32, stuck_at: u32 },
}

impl std::fmt::Display for ReadOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadOnly::NoMigrationPath { found, stuck_at } => write!(
                f,
                "this document was written by schema version {found} and there is no \
                 migration from version {stuck_at}. It can be viewed and exported as a \
                 new document, but not saved over"
            ),
        }
    }
}

/// A document that opened, plus everything §6.3 says must not be lost on the way in.
#[derive(Clone, Debug)]
pub struct Loaded {
    document: Document,
    read_only: Option<ReadOnly>,
    notices: Vec<Notice>,
}

impl Loaded {
    /// Read access, always available.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Everything the user has to be told. May be empty.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    pub fn read_only(&self) -> Option<&ReadOnly> {
        self.read_only.as_ref()
    }

    /// The owned document, or the reason §6.3 refused to give you one.
    ///
    /// The save path takes an owned [`Document`], so this is the only door to it and
    /// read-only is a compile-time fact rather than a convention.
    pub fn into_writable(self) -> Result<Document, ReadOnly> {
        match self.read_only {
            Some(reason) => Err(reason),
            None => Ok(self.document),
        }
    }
}

/// Parse a sidecar, applying §6.3 in full.
pub fn from_json(text: &str) -> Result<Loaded, DocumentError> {
    from_json_with(text, MIGRATIONS)
}

/// As [`from_json`], with the migration registry supplied.
///
/// The seam exists so §6.3's composition rules can be tested against a synthetic
/// chain. `MIGRATIONS` is empty today and will be for a while, and a machine that is
/// only exercised by its own emptiness is not exercised.
pub fn from_json_with(text: &str, registry: &[Migration]) -> Result<Loaded, DocumentError> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| DocumentError::Syntax(e.to_string()))?;

    let mut notices = Vec::new();
    let mut read_only = None;

    match migrate::migrate(&mut value, registry) {
        Ok(Migrated::Current) => {}
        Ok(Migrated::Applied { from, to, steps }) => {
            notices.push(Notice::Migrated { from, to, steps });
        }
        // Read-only rather than refused: the user's edits are still in there, and
        // §6.3 would rather show them and offer export-as-new than lose them behind
        // an error dialogue.
        Err(MigrateError::NoPathFrom { found, stuck_at }) => {
            read_only = Some(ReadOnly::NoMigrationPath { found, stuck_at });
        }
        Err(e) => return Err(DocumentError::Migrate(e)),
    }

    let document: Document =
        serde_json::from_value(value).map_err(|e| DocumentError::Schema(e.to_string()))?;

    super::validate::check(&document).map_err(DocumentError::Invalid)?;

    // Never silently re-rendered: the document keeps its own pipeline version, and
    // the user is told rather than corrected.
    if document.pipeline_version < PIPELINE_VERSION {
        notices.push(Notice::PipelineIsOlder {
            document: document.pipeline_version,
            current: PIPELINE_VERSION,
        });
    }

    Ok(Loaded {
        document,
        read_only,
        notices,
    })
}
