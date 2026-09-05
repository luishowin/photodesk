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

---

## 2026-09-05 — Spike A concluded; spec v0.4 → v0.5

**The first entry where something moved because of evidence rather than argument.** Spike A ran as a source audit of RapidRAW 1.6.3. Its gate failed. Deliverable: `FORK-AUDIT.md`.

### Resolved

**`Fork RapidRAW` → FROZEN: do not fork.** Two of four gate criteria fail.

Criterion 4 is decisive and was the criterion added in v0.4 precisely because the other three could pass while the contract quietly went missing. It did exactly that. RapidRAW's per-pixel chain is a single `@compute` entry point — `shader.wgsl:1616`, running to 1910 — in which stage order is the literal statement order. There is no ordering parameter; the file contains no occurrence of `order`, `sequence`, `stage`, `pass_index` or `pipeline`. §13 declares vendored shaders read-only, so **"pipeline order is explicit and versioned" is unimplementable inside the fork.** The order that is there is also not adoptable: noise reduction and sharpening run before exposure, white balance runs after it, and the tone curve runs *after* the display encode, on sRGB values rather than in linear light. That last one is not a reordering, it is a different operator.

Criterion 3 fails 1 of 3: RAW decode and demosaic classify `ADAPT`, but both do so because the code in question is a 257-line wrapper over `rawler` — a crate obtainable without forking anything — and the core shader chain classifies `REPLACE`.

Criterion 2 passes on its wording and should not have. There is a single clean funnel, `get_all_adjustments_from_json → AllAdjustments`, 16 call sites, no shader edit required. But `AllAdjustments` is a flat parameter block that **sums masked parameters into scalars and runs the pipeline once**, where §5 runs the pipeline per layer and composites. Under that model `opacity` has nothing to attenuate, §8's frozen "masks read the layer's input" has no layer input to refer to, and layer order is discarded because summation commutes. Recorded in `FORK-AUDIT.md` as a gate-design note: a criterion phrased over a mechanism is satisfied by any sufficiently trivial adapter, and should be phrased over the invariant instead.

Criterion 1 passes on the merits. `GpuProcessor::new` and `::run` mention no Tauri type; the coupling sits entirely in thin caching wrappers above them. It is the only criterion that passes cleanly, and it turned out not to matter.

**`Build against rawler + libheif + our own shaders` → FROZEN.** The §2.1 fallback, now the path.

### Measured, and larger than the gate

Three findings that each would have surfaced in month two or three:

- **There is no colour management in RapidRAW. None.** Across 35,601 lines of Rust, `icc`, `lcms`, `colorspace`, `color_profile`, `display-p3`, `prophoto` and `chromatic adaptation` return one substantive cluster — Rec.2020 primaries matrices used internally by the AgX tonemapper. No ICC parser, no embedded-profile extraction, no P3 path. The shader does `srgb_to_linear` in and `linear_to_srgb` out, unconditionally, and export hard-codes the EXIF ColorSpace tag to sRGB. §4 would have been greenfield inside the fork.
- **It cannot open HEIF.** `NON_RAW_EXTENSIONS` has no `heic`, `heif` or `avif`, and there is no `libheif` in the dependency tree. §1's native subject and §14's entire v0.1 deliverable are a format the fork base does not read.
- **On Linux its preview is a per-frame JPEG round-trip.** `use_wgpu_renderer` is hard-`false` on Linux; the surface is hard-`None`; `display.wgsl`, the only `@fragment` shader in the project, is behind a `cfg` that excludes Linux. Every interactive frame goes GPU → CPU readback → mozjpeg at quality 65–85 → IPC. That is the opposite end of the trade from §7.2's "zero per-frame IPC", and it means **the fork contributed nothing to the preview path.** Spikes A and C were less coupled than §2.3 assumed.

Also recorded, and cheaper but not free: 35,601 lines of Rust with **zero** `#[test]` and no `tests/` directory; a `.rrdata` sidecar whose `version` field is written and never read, parsed with `unwrap_or_default()` so that a malformed or future-schema file silently becomes an empty document — the exact behaviour §6.3 forbids by name, found in the wild; cache keys built on `DefaultHasher`, which carries no cross-release stability guarantee; and masks rasterised on the CPU rather than the GPU as §8 specifies.

### Reframed

**`Preview renderer path` stays PROVISIONAL → Spike C, and its expected outcome flips back.** §2.3 argued the hand-written-GLSL fallback was the expected outcome because the chain to be transpiled was compute end to end. That chain was RapidRAW's. Authoring fragment-first — `@fragment` entry points, uniform buffers, sampled textures, framebuffer attachments — never uses the constructs GLSL ES 3.0 lacks, so §7.2's naga transpilation is a live branch again. Two things make it fit rather than merely permit it: §5's per-layer loop means a pass carries one layer's parameters, which sits far inside GLES3's 16 KB uniform-buffer minimum where RapidRAW's 32-slot block would not; and §7.3 already required stages 2–9 fused into one pass, which *is* a fragment shader. **This is a hypothesis, and Spike C must run naga against a real shader rather than assume either outcome.**

### Re-pointed

**`Front-end framework` stays PROVISIONAL, exit changed from Spike A to Spike C.** §16 #8 said a fork inherits React + Vite and no fork leaves it open. There is no fork, so Spike A's exit is spent without resolving the item — which §0 forbids. Re-pointed at Spike C, the first thing that will actually put pixels in a webview and therefore the first thing with an opinion. Measured for the record: RapidRAW's front end is 53,160 lines of React 19 + TypeScript 6 + Vite 8 + Tailwind 4 + Zustand 5, none of which is now inherited.

### Sequencing

**The licensing review comes off the critical path, and the specification-only constraint lifts.** The review was sequenced to gate Spike A's conclusion because the fork decision commits months of work and this repository is public. The engineering gate failed first: nothing is vendored, adapted or redistributed. `FORK-AUDIT.md` quotes identifiers and line numbers and copies no source. "Architectural reference" — reading their code and then writing ours — remains a distinct question worth raising if a review still happens, and is not an engineering decision. Nothing here is legal advice.

### Estimate

Phase 0 moves from 2 weeks to **3 weeks**, adopting the number the 2026-09-05 review called the honest one. Spike A took a day rather than the week implied, but it was a static audit that no longer has to be followed by a fork integration — and the two remaining spikes both grew work: Spike B now proves a colour architecture with no incumbent behind it, and Spike C now has an extra branch to test before it can fall back.
