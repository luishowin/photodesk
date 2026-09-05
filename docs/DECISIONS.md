# Decisions

Append-only. Each entry records what moved between states in the §0 register, what resolved it, and when. The register in `ARCHITECTURE.md` is the current state; this file is the history.

Newest last.

---

## 2026-09-05 — spec v0.3 → v0.4

**Nothing moved to FROZEN from a spike. Phase 0 has not started.** This entry records corrections and newly-written-down decisions, not resolved unknowns.

### Corrections

Four cross-references pointed at §13 (Repository) where they meant §12 (Testing). Two version numbers disagreed with the roadmap: the reorder-policy deadline (§5 said before v0.2, §16 said before v0.3 — settled on **v0.2**, since that is the release that introduces presets and copy/paste and therefore starts multiplying saved documents), and ratings (§11 said v0.6, the roadmap said v0.7 — settled on **v0.7**, since v0.6 is the RAW prefix). `1` was bound to both 100%-zoom and a star rating; ratings are now scoped to the filmstrip and grid, which is the context that arrives with v0.7 anyway. A three-cell row in a two-column table became two rows. "Pacific UI" appeared once, was never defined, and is gone.

### Ambiguities resolved

Five places where the spec said two different things, or said nothing where silence would be chosen for it:

- **§5 described two execution models.** The listing made per-layer application a *stage* after stages 2–12, while the prose said a global adjustment is just a layer with `mask: null`. The prose was right. Stages 2–12 are now the loop body and the stack is the loop.
- **§2.1 gained a fourth gate criterion:** stage order must be expressible without editing vendored shaders. The original three could all pass while leaving "pipeline order is explicit and versioned" — a FROZEN item — unimplementable, because order lives inside a shader chain that §13 declares read-only.
- **§13 listed two shader sources.** `engine/` (vendored) and `shaders/photodesk/` ("source of truth") cannot both render. Spike A now returns exactly one answer and the losing directory does not exist.
- **§12.2 would have failed by construction.** Spatial stages are resolution-dependent and grain is not downsample-invariant at all. Radii are now declared in source-relative units, grain is deterministically seeded, and grain alone is carved out of the comparison — with a same-resolution reproducibility test covering what the carve-out can no longer see.
- **§8 did not say which pixel state `luminance` and `color` masks read.** They read the layer's input.

### Newly frozen

`Masks read the layer's input` · `Spatial radii source-relative, grain seeded and carved out` · `History is session-only` · `Cache paths never written into the document` · `Providers run on a worker pool`

Each is an interface or an invariant rather than an implementation choice, which is what §0 says to freeze early. None required a spike; all five were cheap to state and expensive to leave to accident.

### Newly provisional

`v1 discards iPhone HDR gain maps` (exit: an HDR display, or the first wanted gain-mapped export) · `Front-end framework` (exit: Spike A)

The gain-map entry exists because §1's thesis is to take the manufacturer's rendering as the starting point, and on an HDR display that rendering *is* the gain-mapped one. Dropping it silently would make the app look broken to the one user it has. Dropping it deliberately, with the reason written down, is fine.

### Measured, not decided

Scouting RapidRAW 1.6.3 established that its shader chain is compute end to end — `@compute` entry points, a `texture_storage_2d<rgba8unorm, write>` output, a storage buffer of adjustments. GLSL ES 3.0 has none of those three constructs, so §7.2's naga transpilation has no target to lower onto. This does not resolve Spike C; it reframes it, and moves the hand-written-GLSL fallback from contingency to expected outcome. Recorded in §2.1 and §2.3.

The same pass measured the fork's coupling: 35,601 lines of Rust, the IPC surface concentrated in `file_management.rs` (43 of the `#[tauri::command]`s), a processing core only lightly Tauri-coupled, and `export_processing.rs` heavily coupled enough to read `REPLACE` on sight. Spike A starts from that table rather than from zero.

### Sequencing change

The AGPL licensing review now gates **Spike A's conclusion**, not redistribution. The fork decision commits months of work, and this repository is public from v0.0.
