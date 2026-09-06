# Migration fixtures

§6.3: *"Migrations are pure functions, one per version step, composed, each with a
test fixture in `tests/fixtures/migrations/`."*

**Empty, and that is the correct state.** `photodesk` schema version 1 is the first
one, so there is no step to migrate from. The machinery that runs them exists and is
tested against a synthetic chain in `src-tauri/tests/document.rs` — a real migration
in the shipped registry with no real customer would be a step in the product written
to make a test pass.

## When the first one lands

One directory per step, named for it, holding a matched pair:

```
0001-to-0002/
├── before.photodesk.json   ← written by the old schema
└── after.photodesk.json    ← what the migration must produce, exactly
```

The test asserts `migrate(before) == after` byte for byte against the canonical
serialisation, so the fixture is the specification of the step rather than an
illustration of it.

Two things the first migration will meet, both already recorded in the tests:

- **Remove keys, do not blank them.** `serde_json::Value::take` leaves the key in
  place holding `null`, and `params` denies unknown fields — so a step that blanks a
  renamed key produces a document the *new* schema rejects, which reads as a schema
  bug rather than a migration bug.
- **Do not stamp the version yourself.** `migrate` writes `photodesk` after the step
  returns, so a step cannot forget to and cannot claim to have landed somewhere it
  did not.
