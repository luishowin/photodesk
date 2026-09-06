//! The document model (§6) — schema, validation, migration.
//!
//! §13 puts `document/` in the TypeScript front end, and the *editing* model belongs
//! there. What lives here is the **schema**: the thing the file format is, the rules
//! §6.1 and §6.3 state about it, and the migrations §6.3 requires. It is in Rust for
//! one reason, recorded in §0's register — §12.3's source-preservation test and
//! §12.1's golden images have to run headless on every commit, and a document model
//! reachable only through a webview cannot be tested that way. That is criterion 1 of
//! the gate Spike A failed RapidRAW on, and failing it ourselves would be worse.
//!
//! The TypeScript types are generated from these declarations rather than written
//! beside them — see `tests/bindings.rs`.

pub mod load;
pub mod migrate;
pub mod params;
pub mod schema;
pub mod validate;

pub use load::{DocumentError, Loaded, Notice, ReadOnly, from_json, from_json_with};
pub use migrate::{MIGRATIONS, MigrateError, Migrated, Migration, migrate};
pub use params::{AdjustV1, Params, ParamsError};
pub use schema::{
    ColorSpace, Crop, Document, Geometry, Layer, Mask, MaskComponent, MaskOp, MetadataPolicy, Op,
    Output, OutputFormat, PIPELINE_VERSION, SCHEMA_VERSION, Source, Stroke,
};

/// The TypeScript types, generated from the declarations above.
///
/// §13 puts the editing model in the front end and §10.3 forbids it a framework, so
/// the panels read and write this document by hand. What they must not do is describe
/// it a second time: a hand-written `Document` interface in TypeScript would be a
/// second schema, and the one that drifts is the one with no test looking at it.
///
/// The arrangement is the one `tests/renderer/` already uses — the WebGL2 harness
/// loads *naga-generated* GLSL rather than a hand-written twin, deliberately, because
/// a twin makes the invariant it is supposed to demonstrate untestable. Same argument,
/// different artefact.
///
/// The generated file **is committed**, unlike the generated GLSL, because the front
/// end builds from it: a fresh clone that cannot run `npm run build` until somebody
/// remembers to run `cargo test` is a worse trade than a file in the tree. Staleness
/// is what the test guards — it rewrites the file and then fails if the contents moved,
/// so the fix is already applied by the time you read the message.
#[cfg(test)]
mod bindings {
    use super::schema::Document;
    use ts_rs::{Config, TS};

    /// Where the front end will import from (§13's `src/document/`).
    const OUT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/document/generated");

    #[test]
    fn typescript_bindings_are_current() {
        let path = std::path::Path::new(OUT_DIR).join("document.ts");
        let before = std::fs::read_to_string(&path).unwrap_or_default();

        let cfg = Config::default().with_out_dir(OUT_DIR);
        Document::export_all(&cfg).expect("export TypeScript bindings");

        let after = std::fs::read_to_string(&path).expect("bindings were not written");
        println!("bindings: {} ({} bytes)", path.display(), after.len());

        // Named types, not `any`. The point of generating them is that the front end
        // gets the schema rather than a shrug.
        for expected in [
            "export type Document",
            "export type Layer",
            "export type AdjustV1",
            r#"export type ColorSpace = "srgb" | "display-p3""#,
        ] {
            assert!(
                after.contains(expected),
                "the generated bindings have no `{expected}` in them"
            );
        }
        assert_eq!(
            before,
            after,
            "\nthe generated TypeScript no longer matches the Rust schema.\n\
             The new file has just been written to {}, so the fix is done — commit it \
             alongside the schema change that caused it.\n",
            path.display()
        );
    }
}
