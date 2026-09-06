//! §6.1's rules and §6.3's table, as tests.
//!
//! §6.3 is a table of six conditions and six behaviours, and three of those behaviours
//! are not "accept" or "reject" — they are accept-with-a-consequence. A table like that
//! is exactly the kind of thing that gets implemented for the two easy rows and
//! remembered for the others, so every row below is named after the row it is.
//!
//! Run with `cargo test -p photodesk -- --nocapture` if you want to read the error
//! messages, which are half the point: §6.3 asks for a *clear message*, and a
//! rejection the user cannot act on is not much better than a crash.

use photodesk::photodesk::document::{
    self, AdjustV1, ColorSpace, Document, DocumentError, Layer, MetadataPolicy, Migrated,
    MigrateError, Migration, Notice, Op, Output, OutputFormat, Params, ReadOnly, SCHEMA_VERSION,
    Source,
};

/// A minimal well-formed sidecar, as JSON, so the tests below can mutate one field at
/// a time and be about that field.
fn valid_json() -> serde_json::Value {
    serde_json::json!({
        "photodesk": SCHEMA_VERSION,
        "pipeline_version": 1,
        "source": {
            "file": "IMG_4821.HEIC",
            "hash": "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "dimensions": [4032, 3024],
            "colorspace": "display-p3",
            "orientation": 1
        },
        "stack": [
            {
                "id": "global",
                "op": "adjust",
                "op_version": 1,
                "enabled": true,
                "mask": null,
                "params": { "exposure": 0.35, "contrast": -4.0 }
            }
        ],
        "output": {
            "format": "jpeg",
            "quality": 92,
            "colorspace": "srgb",
            "metadata": "keep-minus-gps"
        }
    })
}

fn load(value: &serde_json::Value) -> Result<document::Loaded, DocumentError> {
    document::from_json(&value.to_string())
}

fn source() -> Source {
    Source {
        file: "IMG_4821.HEIC".into(),
        hash: "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        dimensions: [4032, 3024],
        colorspace: ColorSpace::DisplayP3,
        orientation: 1,
    }
}

// ------------------------------------------------------------------- §6.1's rules

#[test]
fn a_well_formed_document_opens_with_nothing_to_report() {
    let loaded = load(&valid_json()).expect("the baseline document must open");
    assert!(loaded.read_only().is_none());
    assert_eq!(loaded.notices(), &[]);

    let doc = loaded.document();
    assert_eq!(doc.stack.len(), 1);
    let Params::AdjustV1(p) = &doc.stack[0].params;
    assert_eq!(p.exposure, Some(0.35));
    assert_eq!(p.contrast, Some(-4.0));
}

/// §6.1: "Unknown keys are a validation error, never a silent no-op."
///
/// The message has to name the key. A rejection that says only "invalid document"
/// leaves the user with a file they cannot fix and no idea which of forty numbers is
/// the problem.
#[test]
fn an_unknown_params_key_is_rejected_by_name() {
    let mut doc = valid_json();
    doc["stack"][0]["params"]["exposre"] = serde_json::json!(0.5);

    let err = load(&doc).expect_err("a typo in params must not be silently ignored");
    let message = err.to_string();
    println!("{message}");
    assert!(
        message.contains("exposre"),
        "the error does not name the offending key: {message}"
    );
    assert!(
        message.contains("global"),
        "the error does not say which layer: {message}"
    );
}

/// §6.1: "Omitted keys mean *identity*, not zero."
///
/// The distinction is only observable through a merge, which is why this test does one
/// — see [`AdjustV1::merge_from`]. If omission collapsed to a default at parse time,
/// the two documents below would be indistinguishable and the assertion at the end
/// would be impossible to write.
#[test]
fn an_omitted_parameter_is_absent_rather_than_zero() {
    let mut doc = valid_json();
    doc["stack"][0]["params"] = serde_json::json!({ "exposure": 0.35 });
    let loaded = load(&doc).expect("open");
    let Params::AdjustV1(base) = loaded.document().stack[0].params;

    assert_eq!(base.exposure, Some(0.35));
    assert_eq!(base.contrast, None, "an omitted contrast is not a contrast of 0");

    // A tool preset that says nothing about exposure must leave exposure alone. This
    // is the operation the `Option` exists for.
    let preset = AdjustV1 {
        contrast: Some(20.0),
        ..Default::default()
    };
    let mut merged = base;
    merged.merge_from(&preset);
    assert_eq!(merged.exposure, Some(0.35), "the merge overwrote a field it never mentioned");
    assert_eq!(merged.contrast, Some(20.0));

    // And the counterexample, so the assertion above is known to be measuring
    // something: a preset that *does* mention exposure replaces it.
    let mut overwritten = base;
    overwritten.merge_from(&AdjustV1 {
        exposure: Some(-1.0),
        ..Default::default()
    });
    assert_eq!(overwritten.exposure, Some(-1.0));
}

/// An identity layer is one that would do nothing (§6.1, §6.2's node elimination).
#[test]
fn an_empty_params_object_is_identity() {
    let mut doc = valid_json();
    doc["stack"][0]["params"] = serde_json::json!({});
    let loaded = load(&doc).expect("an all-identity layer is well formed, just useless");
    assert!(loaded.document().stack[0].params.is_identity());
    assert!(!load(&valid_json()).unwrap().document().stack[0].params.is_identity());
}

/// §0, frozen: **cache paths are never written into the document.**
#[test]
fn a_document_may_not_point_into_the_cache() {
    let mut doc = valid_json();
    doc["source"]["file"] = serde_json::json!(".photodesk/proxy/IMG_4821.png");

    let err = load(&doc).expect_err("a document naming a disposable file must be refused");
    println!("{err}");
    assert!(
        err.to_string().contains(".photodesk/"),
        "the error does not show what it objected to: {err}"
    );

    // The rule is checked over the whole serialised document rather than field by
    // field, so a cache path anywhere is caught — including in a field added later.
    let mut doc = valid_json();
    doc["stack"][0]["name"] = serde_json::json!("copied from .photodesk/masks/sky.png");
    assert!(load(&doc).is_err());
}

/// §0 makes the document human-readable, which makes it diffable, which only means
/// anything if writing it twice produces the same bytes.
///
/// The stronger claim this supports: **opening a photograph does not modify its
/// sidecar.** That is a byte-level promise, not a semantic one, and it only holds if
/// the serialisation is canonical.
#[test]
fn a_document_round_trips_byte_for_byte() {
    let original = load(&valid_json()).unwrap().into_writable().unwrap();
    let once = photodesk::photodesk::sidecar::to_canonical_json(&original).unwrap();
    let reparsed = document::from_json(&once).unwrap().into_writable().unwrap();
    let twice = photodesk::photodesk::sidecar::to_canonical_json(&reparsed).unwrap();

    assert_eq!(once, twice, "serialisation is not canonical");
    assert_eq!(original, reparsed, "the document did not survive its own round trip");

    // Absent fields stay absent. A loader that materialises defaults would write them
    // back on the next save, and every sidecar would grow keys nobody set.
    let mut doc = valid_json();
    doc["stack"][0]["params"] = serde_json::json!({ "exposure": 0.35 });
    let text = photodesk::photodesk::sidecar::to_canonical_json(
        &load(&doc).unwrap().into_writable().unwrap(),
    )
    .unwrap();
    println!("{text}");
    assert!(!text.contains("contrast"), "an unset parameter was written out:\n{text}");
    assert!(!text.contains("geometry"), "an identity geometry was written out:\n{text}");
}

/// §6.1's default output, which is a privacy decision as much as a format one.
#[test]
fn a_new_document_defaults_to_srgb_without_gps() {
    let doc = Document::new(source());
    assert_eq!(doc.photodesk, SCHEMA_VERSION);
    assert_eq!(doc.output.colorspace, ColorSpace::Srgb);
    assert_eq!(doc.output.metadata, MetadataPolicy::KeepMinusGps);
    // §5 skips identity stages, so a freshly opened photograph has no layers at all
    // rather than one that does nothing.
    assert!(doc.stack.is_empty());
}

// -------------------------------------------------------------- §6.3, row by row

/// §6.3 row 1: "`photodesk` newer than app → **Reject.** Clear message. Never guess at
/// a future schema."
#[test]
fn row_1_a_document_from_the_future_is_rejected() {
    let mut doc = valid_json();
    doc["photodesk"] = serde_json::json!(SCHEMA_VERSION + 1);

    let err = load(&doc).expect_err("a newer schema must not be guessed at");
    println!("{err}");
    let message = err.to_string();
    assert!(message.contains(&(SCHEMA_VERSION + 1).to_string()));
    assert!(
        message.to_lowercase().contains("upgrade"),
        "the message does not tell the user what to do: {message}"
    );
}

/// §6.3 row 2: "`photodesk` older, migration exists → Migrate on load, write back on
/// next save."
#[test]
fn row_2_an_older_document_with_a_migration_is_migrated_on_load() {
    // A synthetic chain, because `MIGRATIONS` is empty and schema 1 is the first.
    // The alternative — a fictional migration in the shipped registry — would put a
    // step in the product to make a test pass.
    const CHAIN: &[Migration] = &[Migration {
        from: 0,
        to: 1,
        apply: |v| {
            // A migration of exactly the shape a real one has: a key was renamed.
            //
            // Note the `remove` rather than a `take`. `Value::take` leaves the key in
            // place holding `null`, and `params` denies unknown fields — so a
            // migration that blanks a key instead of deleting it produces a document
            // the *new* schema rejects, which reads as a schema bug rather than a
            // migration bug. The first real migration will meet this.
            if let Some(params) = v["stack"][0]["params"].as_object_mut()
                && let Some(old) = params.remove("ev")
            {
                params.insert("exposure".into(), old);
            }
            Ok(())
        },
    }];

    let mut doc = valid_json();
    doc["photodesk"] = serde_json::json!(0);
    doc["stack"][0]["params"] = serde_json::json!({ "ev": 1.5 });

    let loaded = document::from_json_with(&doc.to_string(), CHAIN).expect("migrate on load");
    assert_eq!(
        loaded.notices(),
        &[Notice::Migrated { from: 0, to: 1, steps: 1 }],
        "a migration the user was not told about is a file rewritten behind their back"
    );
    let out = loaded.into_writable().expect("a migrated document is writable");
    assert_eq!(out.photodesk, SCHEMA_VERSION);
    let Params::AdjustV1(p) = out.stack[0].params;
    assert_eq!(p.exposure, Some(1.5), "the migration did not carry the value across");
}

/// §6.3 row 3: "`photodesk` older, no migration → Open **read-only**, explain, offer
/// export-as-new."
///
/// Read-only is enforced by the type rather than by a flag: the only door to an owned
/// `Document` is `into_writable`, and the save path takes one.
#[test]
fn row_3_an_unmigratable_document_opens_read_only() {
    let mut doc = valid_json();
    doc["photodesk"] = serde_json::json!(0);

    let loaded = load(&doc).expect("it opens — the edits are still in there");
    let reason = loaded.read_only().expect("it must not be writable").clone();
    println!("{reason}");
    assert!(matches!(reason, ReadOnly::NoMigrationPath { found: 0, stuck_at: 0 }));
    assert!(
        reason.to_string().contains("export"),
        "the message does not offer the way out §6.3 promises: {reason}"
    );

    // Still readable, which is the whole point of not rejecting it.
    assert_eq!(loaded.document().stack.len(), 1);
    assert!(loaded.into_writable().is_err());
}

/// §6.3 row 4: "`pipeline_version` older → Open, warn that appearance may differ, offer
/// explicit re-render on current pipeline. **Never silently re-render.**"
#[test]
fn row_4_an_older_pipeline_warns_and_is_never_silently_re_rendered() {
    let mut doc = valid_json();
    doc["pipeline_version"] = serde_json::json!(0);

    let loaded = load(&doc).expect("it opens");
    assert_eq!(
        loaded.notices(),
        &[Notice::PipelineIsOlder { document: 0, current: 1 }]
    );

    // The document keeps its own pipeline version. Bumping it here would *be* the
    // silent re-render — the appearance would change on the next render and the file
    // would no longer say it used to look different.
    let out = loaded.into_writable().expect("it is writable; only the appearance is in doubt");
    assert_eq!(out.pipeline_version, 0);
}

/// §6.3 row 5, first half: "Unknown `op` → reject the document."
#[test]
fn row_5_an_unknown_op_is_rejected() {
    let mut doc = valid_json();
    doc["stack"][0]["op"] = serde_json::json!("deblur");

    let err = load(&doc).expect_err("an op this build does not implement must not be skipped");
    println!("{err}");
    assert!(err.to_string().contains("deblur"));
}

/// §6.3 row 5, second half: "Unknown `op_version` → reject the document."
///
/// The interesting half. An unknown *op* is a word we do not recognise; an unknown
/// *op_version* is a word we do recognise carrying parameters we do not, which is
/// exactly the case where guessing looks harmless.
#[test]
fn row_5_an_unknown_op_version_is_rejected() {
    let mut doc = valid_json();
    doc["stack"][0]["op_version"] = serde_json::json!(2);

    let err = load(&doc).expect_err("adjust v2 does not exist yet");
    println!("{err}");
    let message = err.to_string();
    assert!(message.contains("op_version 2"), "{message}");
    assert!(
        message.contains("partially-understood"),
        "the message does not say why refusing beats guessing: {message}"
    );
}

/// §6.3 row 6 is [`an_unknown_params_key_is_rejected_by_name`]; this is the sibling
/// case serde catches for free, kept because "the schema is closed" has to be true of
/// the whole document rather than only of `params`.
#[test]
fn row_6_an_unknown_key_anywhere_is_rejected() {
    for (path, value) in [
        ("colour_profile", serde_json::json!("srgb")),
        ("notes", serde_json::json!("hello")),
    ] {
        let mut doc = valid_json();
        doc[path] = value;
        let err = load(&doc).expect_err("an unknown top-level key must be refused");
        assert!(err.to_string().contains(path), "{err}");
    }
}

// ------------------------------------------------------- migration composition

/// Steps run in order, stop at the target, and stamp the version themselves.
#[test]
fn migrations_compose_in_order_and_stop_at_the_current_version() {
    fn bump(v: &mut serde_json::Value) -> Result<(), String> {
        let trail = v["trail"].as_str().unwrap_or("").to_string();
        v["trail"] = serde_json::json!(format!("{trail}."));
        Ok(())
    }
    // Two steps to reach 1 from -1 would be nonsense; the chain is 0 -> 1 with a
    // redundant 1 -> 2 that must not run, because the target is SCHEMA_VERSION.
    const CHAIN: &[Migration] = &[
        Migration { from: 0, to: 1, apply: bump },
        Migration { from: 1, to: 2, apply: bump },
    ];

    let mut v = serde_json::json!({ "photodesk": 0 });
    let outcome = document::migrate(&mut v, CHAIN).expect("migrate");
    assert_eq!(outcome, Migrated::Applied { from: 0, to: 1, steps: 1 });
    assert_eq!(v["photodesk"], 1, "the version was not stamped");
    assert_eq!(v["trail"], ".", "the 1 -> 2 step ran past the target");

    // Already current: nothing runs at all.
    let mut v = serde_json::json!({ "photodesk": SCHEMA_VERSION });
    assert_eq!(document::migrate(&mut v, CHAIN).unwrap(), Migrated::Current);
    assert!(v.get("trail").is_none());
}

/// A gap in the chain is not a silent stop.
#[test]
fn a_missing_migration_step_is_a_gap_rather_than_a_shrug() {
    const CHAIN: &[Migration] = &[Migration {
        from: 5,
        to: 6,
        apply: |_| Ok(()),
    }];
    let mut v = serde_json::json!({ "photodesk": 0 });
    assert_eq!(
        document::migrate(&mut v, CHAIN),
        Err(MigrateError::NoPathFrom { found: 0, stuck_at: 0 })
    );
}

/// A failing step leaves the document exactly as it was.
///
/// A half-migrated document is worse than an unmigrated one, because it looks
/// readable — the version says current and the shape is from two schemas ago.
#[test]
fn a_failed_migration_leaves_the_document_untouched() {
    const CHAIN: &[Migration] = &[Migration {
        from: 0,
        to: 1,
        apply: |v| {
            v["damage"] = serde_json::json!(true);
            Err("the thing this step needed was not there".into())
        },
    }];
    let before = serde_json::json!({ "photodesk": 0, "stack": [] });
    let mut v = before.clone();

    let err = document::migrate(&mut v, CHAIN).expect_err("the step failed");
    assert!(matches!(err, MigrateError::StepFailed { from: 0, to: 1, .. }));
    assert_eq!(v, before, "a failed migration modified the document anyway");
}

// ---------------------------------------------------------------- consistency

/// §9.3 derives cache keys from the layer id and §12.2 seeds grain from it, so two
/// layers sharing one alias. The symptom is one layer's noise appearing on another,
/// which nobody would trace back to a duplicate string in a sidecar.
#[test]
fn duplicate_layer_ids_are_rejected() {
    let mut doc = valid_json();
    let layer = doc["stack"][0].clone();
    doc["stack"] = serde_json::json!([layer.clone(), layer]);

    let err = load(&doc).expect_err("two layers cannot share an id");
    println!("{err}");
    assert!(err.to_string().contains("global"));
}

/// JSON cannot represent a NaN, so one in a document came from a writer that was
/// already wrong — and it would propagate through every later stage into the file.
#[test]
fn a_non_finite_parameter_is_rejected() {
    let doc = Document {
        stack: vec![Layer {
            id: "global".into(),
            op: Op::Adjust,
            op_version: 1,
            enabled: true,
            name: None,
            opacity: None,
            mask: None,
            params: Params::AdjustV1(AdjustV1 {
                exposure: Some(f32::NAN),
                ..Default::default()
            }),
        }],
        ..Document::new(source())
    };
    let err = document::validate::check(&doc).expect_err("NaN must not reach the pipeline");
    println!("{err}");
    assert!(err.contains("exposure"));
}

/// The rules that make a document *coherent*, each stated where it is checked.
#[test]
fn incoherent_documents_are_rejected_with_a_reason() {
    let cases: [(&str, Box<dyn Fn(&mut serde_json::Value)>, &str); 7] = [
        (
            "a hash with no algorithm cannot be re-verified by §12.3",
            Box::new(|d| d["source"]["hash"] = serde_json::json!("9f2a")),
            "algorithm",
        ),
        (
            "an absolute source path stops being true the first time the photo moves",
            Box::new(|d| d["source"]["file"] = serde_json::json!("/home/me/IMG.HEIC")),
            "absolute",
        ),
        (
            "a source path may not escape its own directory",
            Box::new(|d| d["source"]["file"] = serde_json::json!("../../etc/passwd")),
            "escapes",
        ),
        (
            "orientation is an EXIF value, 1 to 8",
            Box::new(|d| d["source"]["orientation"] = serde_json::json!(9)),
            "orientation",
        ),
        (
            "a crop is in normalised coordinates and has to have area",
            Box::new(|d| {
                d["geometry"] = serde_json::json!({"crop": {"x": 0.5, "y": 0.0, "w": 0.9, "h": 1.0}})
            }),
            "unit square",
        ),
        (
            "opacity is a fraction",
            Box::new(|d| d["stack"][0]["opacity"] = serde_json::json!(1.5)),
            "opacity",
        ),
        (
            "jpeg with no quality cannot be exported twice the same way",
            Box::new(|d| {
                d["output"]
                    .as_object_mut()
                    .unwrap()
                    .remove("quality");
            }),
            "quality",
        ),
    ];

    for (why, mutate, expected) in cases {
        let mut doc = valid_json();
        mutate(&mut doc);
        let err = load(&doc).unwrap_err();
        println!("{why}\n  -> {err}\n");
        assert!(
            err.to_string().contains(expected),
            "{why}: the error does not mention `{expected}`: {err}"
        );
    }
}

/// A `Document` assembled in memory can say one thing in `op`/`op_version` and hold
/// another in `params`. On load they agree by construction; this is the other path.
#[test]
fn a_layer_whose_op_disagrees_with_its_params_is_rejected() {
    let doc = Document {
        stack: vec![Layer {
            id: "global".into(),
            op: Op::Adjust,
            op_version: 7,
            enabled: true,
            name: None,
            opacity: None,
            mask: None,
            params: Params::AdjustV1(AdjustV1::default()),
        }],
        ..Document::new(source())
    };
    let err = document::validate::check(&doc).expect_err("v7 params do not exist");
    println!("{err}");
    assert!(err.contains("v7"));
}

/// Masks are a v0.4 feature and a v0.1 concern all the same: the sidecar is a file
/// format, and a document a later version writes has to be one this version can at
/// least read. §6.3 gives "parses but is not rendered" a defined behaviour and gives
/// "does not parse" none.
#[test]
fn a_masked_layer_parses_at_v0_1_even_though_nothing_renders_it_yet() {
    let mut doc = valid_json();
    doc["stack"][0]["mask"] = serde_json::json!({
        "op": "union",
        "components": [
            { "type": "ai", "kind": "sky", "feather": 12.0 },
            { "type": "linear", "from": [0.5, 0.0], "to": [0.5, 0.42] }
        ]
    });
    let loaded = load(&doc).expect("a v0.4 document must at least open");
    let mask = loaded.document().stack[0].mask.as_ref().expect("mask");
    assert_eq!(mask.components.len(), 2);

    // And the component set is closed, so a mask kind from the future is refused
    // rather than dropped.
    doc["stack"][0]["mask"]["components"] = serde_json::json!([{ "type": "depth" }]);
    assert!(load(&doc).is_err());
}

/// The defaults §6.1 states, checked where they are easy to get wrong: `enabled` is
/// absent-means-true, which is the opposite of what a `bool`'s own default would give.
#[test]
fn an_absent_enabled_flag_means_enabled() {
    let mut doc = valid_json();
    doc["stack"][0].as_object_mut().unwrap().remove("enabled");
    let loaded = load(&doc).expect("open");
    assert!(
        loaded.document().stack[0].enabled,
        "a layer with no `enabled` key became disabled, which silently drops an edit"
    );
}

/// Not a rule — a guard on the shape of the type. `Output` is serialised into every
/// sidecar, so an accidental change to its field set is a change to the file format.
#[test]
fn the_output_block_is_what_section_6_1_shows() {
    let output = Output {
        format: OutputFormat::Jpeg,
        quality: Some(92),
        colorspace: ColorSpace::Srgb,
        metadata: MetadataPolicy::KeepMinusGps,
    };
    let json = serde_json::to_value(&output).unwrap();
    let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["format", "quality", "colorspace", "metadata"]);
    assert_eq!(json["metadata"], "keep-minus-gps");
}
