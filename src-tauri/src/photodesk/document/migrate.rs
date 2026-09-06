//! §6.3's migrations: pure functions, one per version step, composed.
//!
//! **The registry is empty, and that is the correct state today** — [`SCHEMA_VERSION`]
//! is 1 and 1 is the first, so there is no step to write. What exists here is the
//! machinery that runs when there is, and the reason it exists before its first
//! customer is that §6.3 gives *five other* conditions defined behaviour — reject a
//! newer schema, open an unmigratable older one read-only, and so on — and those are
//! all decisions about a document the app cannot fully understand. Writing them at
//! the moment the first migration lands means writing them under pressure, next to a
//! schema change, with a user's edits in the balance.
//!
//! **Migrations run on raw JSON, before typing.** They have to: a document written by
//! a future schema version cannot be parsed by this version's types, which is the
//! whole reason it needs migrating. So a step takes a `serde_json::Value` and returns
//! one, and typing happens once afterwards.
//!
//! **The chain is a parameter, not a global.** [`migrate`] takes the registry it
//! should apply, so a test can compose a synthetic chain and check that steps run in
//! order, stop at the target, and refuse a gap — without a fictional migration
//! sitting in the shipped registry pretending to have a customer.

use serde_json::Value;

use super::schema::SCHEMA_VERSION;

/// One version step. Pure: it may not read the filesystem, the clock, or anything
/// else that would make the same document migrate differently twice.
#[derive(Clone, Copy)]
pub struct Migration {
    pub from: u32,
    pub to: u32,
    pub apply: fn(&mut Value) -> Result<(), String>,
}

impl std::fmt::Debug for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Migration({} -> {})", self.from, self.to)
    }
}

/// Every migration this build knows, in ascending order.
///
/// Empty. See the module note — schema version 1 is the first one.
pub const MIGRATIONS: &[Migration] = &[];

/// What happened to a document on the way in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Migrated {
    /// Already current. Nothing ran.
    Current,
    /// Migrated up. §6.3: "Migrate on load, write back on next save" — so the caller
    /// is told which steps ran, because a silent rewrite of somebody's sidecar is a
    /// change they did not ask for and cannot see.
    Applied { from: u32, to: u32, steps: u32 },
}

/// Why a document could not be brought to the current schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrateError {
    /// §6.3: "`photodesk` newer than app → **Reject.** Clear message. Never guess at a
    /// future schema."
    FromTheFuture { found: u32, current: u32 },
    /// §6.3: "`photodesk` older, no migration → Open **read-only**, explain, offer
    /// export-as-new." Not an error to the user; an error to *this* function, whose
    /// job is only to produce a current document.
    NoPathFrom { found: u32, stuck_at: u32 },
    /// A step ran and failed. The document is left untouched — see [`migrate`].
    StepFailed { from: u32, to: u32, error: String },
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrateError::FromTheFuture { found, current } => write!(
                f,
                "this document is schema version {found}; this build understands {current}. \
                 Upgrade PhotoDesk to open it — guessing at a future schema is how edits \
                 get silently dropped"
            ),
            MigrateError::NoPathFrom { found, stuck_at } => write!(
                f,
                "no migration from schema version {stuck_at} (document is {found}). It can \
                 be opened read-only and exported as a new document"
            ),
            MigrateError::StepFailed { from, to, error } => {
                write!(f, "migration {from} -> {to} failed: {error}")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

/// Bring `value` up to [`SCHEMA_VERSION`] by composing `registry` in order.
///
/// `value` is modified in place **only on success**: a partially migrated document is
/// worse than an unmigrated one, because it looks readable. The work happens on a
/// clone and is committed at the end.
pub fn migrate(value: &mut Value, registry: &[Migration]) -> Result<Migrated, MigrateError> {
    let found = value
        .get("photodesk")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    if found > SCHEMA_VERSION {
        return Err(MigrateError::FromTheFuture {
            found,
            current: SCHEMA_VERSION,
        });
    }
    if found == SCHEMA_VERSION {
        return Ok(Migrated::Current);
    }

    let mut working = value.clone();
    let mut at = found;
    let mut steps = 0u32;

    while at < SCHEMA_VERSION {
        let Some(step) = registry.iter().find(|m| m.from == at) else {
            return Err(MigrateError::NoPathFrom { found, stuck_at: at });
        };
        (step.apply)(&mut working).map_err(|error| MigrateError::StepFailed {
            from: step.from,
            to: step.to,
            error,
        })?;
        // The step is responsible for the document's shape; the version is stamped
        // here so a step cannot forget to, and cannot lie about where it landed.
        working["photodesk"] = Value::from(step.to);
        at = step.to;
        steps += 1;
    }

    *value = working;
    Ok(Migrated::Applied {
        from: found,
        to: SCHEMA_VERSION,
        steps,
    })
}
