# PhotoDesk — working notes

A display-referred photo editor for Fedora / GNOME. Read `docs/ARCHITECTURE.md` before proposing anything; it is the master spec and it is unusually prescriptive on purpose.

The only code in the tree is Phase 0's two harnesses, `tests/color/` and `tests/renderer/`. There is no product code, and that is deliberate — §14 gives v0.0 none.

## How this project works

The spec's §0 register is the contract. Every item is either **FROZEN** (with a written reason) or **PROVISIONAL** (with a named spike that resolves it). Anything not in the register is undecided, and a decision must be recorded there before code depends on it.

Two rules govern changes to it:

- **Don't freeze without a reason you can write in a sentence.** If it can't be justified, it isn't frozen, it's a habit.
- **Don't mark something provisional without naming its exit condition.** "Provisional" with no exit is indecision with better PR.

When something moves between states, append to `docs/DECISIONS.md` — what moved, what resolved it, the date. The register is the current state; that file is the history. Don't edit past entries.

## Current state

v0.0, **Phase 0 complete**. Spec is at v0.16. All three spikes have run and two of the three answers were not the expected ones.

**Spike A** (2026-09-05) audited RapidRAW 1.6.3 and returned *do not fork*. Read `docs/FORK-AUDIT.md` before revisiting anything about the render path. Short version: the per-pixel chain is one `@compute` kernel in which stage order is the literal statement order, and vendored shaders are read-only, so the frozen "pipeline order is explicit and versioned" was unimplementable in a fork. Separately, RapidRAW has no colour management at all, cannot open HEIF, and on Linux ships every preview frame as a lossy JPEG over IPC.

**Spike B** (2026-09-05) is green. `docs/SPIKE-B.md`, harness at `tests/color/`. The working space is frozen as linear Display P3 f16: it costs ΔE 0.0956 at thirty passes against a budget of 1.0, and the deep-shadow ramp is bit-exact. §2.2's f32 fallback will not be built.

**Spike C** (2026-09-05) is green, and reversed Spike A's apparent verdict on §7.2. `docs/SPIKE-C.md`, harness at `tests/renderer/`. Authored fragment-first, the WGSL lowers to GLSL ES 3.00, WebKitGTK compiles it, six layers cost 5.92 ms at 2 MP against 16 ms, and the WebGL2 and wgpu paths agree to max 0.0088 across 196,608 samples. `EXT_color_buffer_float` is present with RGBA16F colour-renderable and linear-filterable, which retires the risk that B and C could each be green and jointly wrong.

**v0.1 is unblocked.** The iPhone HEIC path is tested end to end — `libheif-freeworld` is installed and `heif_codecs.rs` now *asserts* HEVC, so a mis-provisioned machine says so rather than failing to open a photograph. Fedora ships libheif without HEVC on patent grounds; that lands on §13 as §16 #13, because a Fedora RPM may not require a third-party repo, and v0.1's decode path should return a distinguishable "no codec" error rather than a generic failure.

**A HEIC costs ~0.9 ΔE before we see it** — RGB↔YCbCr conversion, identical across libaom and x265 at lossless, so it is inherent and unavoidable on read. §12.1's HEIC thresholds must clear it (§16 #14).

**§4 is verified against real photographs.** An iPhone HEIC and JPEG both tag Display P3 with the same 536-byte profile, the base image is 8-bit, and a real photograph round-trips through linear P3 f16 at max ΔE 0.0000. The gain map is a half-resolution auxiliary image and libheif's default decode ignores it, so v1's discard is a visible skip. **The photographs are not in the repo and must not be** — `real_photos.rs` reads `PHOTODESK_CORPUS_DIR` (default `~/Downloads`) and skips when empty, and reports EXIF by type and size only, never by value.

**§16 #11 is closed: the export gamut policy is `clip chroma at constant luminance`** (2026-09-06, §4, `tests/color/tests/gamut_policy.rs`). It slides a colour along the ray from the achromatic point of its own luminance to the gamut boundary, so L\* is exact and chroma is the only thing spent. Chosen over the clip because *the clip's error has no policy* — how much lightness is lost depends on which channel ran out first, up to 2.8 L\* — and because it halves the adjacent pixel pairs a real photograph merges into one colour, at zero cost inside the gamut. The soft-knee variant is better at gradients and was rejected: it moves 3.2% of an already-correct frame on the strength of a tuning constant with no derivation. It is implemented and swept (`GamutPolicy::CompressLuma`) so reopening it is one line.

Two things it dragged in. **Stage 13 now has a shader** — `tests/renderer/shaders/encode.wgsl`, checked to lower to GLSL ES 3.00 and to agree with the Rust reference to one colour-attachment step. And **RGBA16F attachments can truncate rather than round** (measured on RADV/RENOIR), which is a full-step bias that §12.1's thresholds have to allow for — §16 #15.

**The preview path is confirmed in Tauri's own webview.** Spike C measured in Epiphany (webkitgtk-6.0, GTK 4); Tauri v2 embeds webkit2gtk-4.1 (GTK 3), and `SPIKE-C.md` left that to the first v0.1 build. `run-probe.py --engine webkit2gtk-4.1` drives the 4.1 WebView directly, and the two bindings return identical capabilities, an identically compiled shader, and pixel agreement **identical to every digit** — max 0.008789, mean 0.0001944. Six layers cost 6.17 ms against 5.92, inside a 16 ms budget either way. **Nothing on the "before v0.1" list remains; the next work is v0.1 itself.**

**v0.1 has started, and the first piece is the document model** (`src-tauri/`, 36 tests). Two register items came with it. **The schema's home is Rust and the TypeScript types are generated from it** — §12.3 and §12.1 must run headless on every commit, which is criterion 1 of the gate Spike A failed RapidRAW on, and a hand-written TS twin would make "one schema" untestable exactly as a twin shader would have made "one shader source" untestable. The generated file is committed to `src/document/generated/`; a test rewrites it and fails if it moved, so staleness arrives with the fix applied. And **the sidecar keeps the whole filename** — `IMG_4821.HEIC.photodesk.json`, correcting §6.1, because v0.1's own workflow puts `IMG_4821.jpg` beside `IMG_4821.HEIC` and the shorter name gives them one sidecar between them.

**`src-tauri/` links no webview and that is deliberate**, not a wait for the four absent devel packages. `tauri` and a `main.rs` arrive when there is a window to open; everything before that has to be testable without one.

**§6.2's graph compile is done** (`src-tauri/src/photodesk/graph/`, 18 tests), and it brought a third register item: **the graph is compiled once, in Rust, and both renderers execute the same plan.** §13 put the compile in the front end — but preview runs in the webview and export runs through wgpu, so two compilers would render two topologies and drift the way two shader sources would, with §12.2 then comparing two *compilations* rather than two executions of one plan. `src/graph/` is the executor.

The four benefits §6.2 calls free come from one thing: a node's key is `H(what it does, keys of its inputs)`. Dedup is a lookup on insertion; the dirty set is key-set subtraction, which answers a reorder, an insertion and a deletion as readily as a slider drag. Two rules from elsewhere are now structural rather than remembered — **feather is a separate node** so §9.3's "feather is not in the key" cannot be violated, and **the node key is scale-free** with resolution folded into `cache_key` only, which is what lets §12.2 run one graph at two scales.

**The front end has no framework** (2026-09-06, §10.3): TypeScript and Vite, zero runtime dependencies. Don't add React or a component library to make a panel easier — §11's slider contract is the reason the decision went this way, and a library's slider would be overridden rather than used. Panels, undo, focus and the keymap are hand-written by design.

**Where the work is.** All of Phase 0 is on **`main`** and pushed (2026-09-06). The `spike-a-fork-audit` branch was fast-forwarded into it and is now redundant — it points at the same history, and its name was left over from when it held only Spike A. Delete it whenever; nothing depends on it.

## Things that will bite

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling. The specification-only constraint has lifted, but it lifted *because* nothing is being copied — don't undo the premise.
- **Never author a compute shader, a storage buffer or a storage texture.** This is now a frozen register item, not a preference. naga refuses all three by name when targeting GLSL ES 3.00, so one of them anywhere breaks the preview path for the whole project. `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns.
- **Both harnesses cross-validate themselves on purpose.** `tests/color/` checks its ΔE2000 and its matrices against lcms2; `tests/renderer/` keeps a negative control that asserts compute is *refused*. Don't delete either as redundant — an error in ΔE2000 would make every colour threshold meaningless and green, and without the control the fragment result is a coincidence rather than a consequence.
- **`src/document/generated/{document,graph}.ts` are generated and committed.** Don't hand-edit them, and don't hand-write a second TypeScript description of either — that is a frozen register item, and the reason is the same one that makes the browser harness load generated GLSL rather than a twin. `cargo test -p photodesk` regenerates them and fails if they changed, so the workflow is: change the Rust, run the tests once, commit both.
- **Don't add lcms2 to the product.** It is a dev-dependency of `tests/color/` and belongs there: the harness needs to be able to *disagree* with the shipped ICC parser, which it cannot do if both call the same library. That is a frozen register item with the same shape as the derived-not-tabulated matrices rule.
- **`NodeKind` variants must serialise under an internal tag.** The key builder hashes the serialised node so a new field is covered automatically, which means a variant serde *cannot* serialise becomes a panic rather than a compile error. A newtype variant holding a string is the shape that does it — `MaskCompose(MaskOp)` compiled, generated plausible TypeScript and failed the first time a two-component mask was keyed. Use struct variants.
- **A green test can quietly change what it measures.** Freezing the gamut policy took Spike B's test 2 from mean ΔE 0.0807 to 1.4183 against its own 1.5 threshold — still passing, and no longer about transform fidelity at all, because its reference converter clips and the pipeline no longer did. It is pinned to `GamutPolicy::ClipLinear` now with the reason next to it. When a policy constant moves, re-read every test whose reference embeds the old one.
- **The photographs are gone from `~/Downloads`** as of this session, so `real_photos.rs` and `gamut_policy.rs`'s real-photograph case both skip. They are personal files and were never in the repo; put one back, or set `PHOTODESK_CORPUS_DIR`, and both run again. The ΔL\*/ΔC\*/ΔH decomposition on real pixels is the one number `DECISIONS.md` still owes.
- **Three spikes in a row produced a confident number that measured the wrong thing**, each caught only by looking at *why* a result had the shape it did: a pass sweep of no-ops, a headroom table destroyed by 8-bit quantisation, a GL error hidden behind a green budget table, and an agreement test fed different inputs on each side. §16 #11 added two more — a ΔE ranking that would have picked the flattest policy, and a shader disagreement that turned out to be the driver's rounding mode. When a measurement comes back suspiciously clean or suspiciously catastrophic, that is the signal to check the instrument first.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page. It is updated **on request only** — do not regenerate it as a side effect of other work. **It is badly stale**: it still shows spec v0.4, all three spikes open, and the licensing review gating Spike A. Three spec versions and a completed Phase 0 behind.
- **Don't add product code to hit a milestone early.** §14 gives v0.0 no product. Two spikes remain, and skipping them is the specific failure the whole document is arranged to prevent. Spike A is the evidence that the arrangement works — it cost a day and saved a fork.
- **`FORK-AUDIT.md`, `SPIKE-B.md` and `SPIKE-C.md` are spike reports, not living documents.** They record what was measured on 2026-09-05. Don't revise them; if something in one turns out wrong, that is a `DECISIONS.md` entry.
- **`tests/renderer/web/generated/` is gitignored and regenerable.** `cargo test -p photodesk-renderer-spike` emits it. The browser harness loads the *generated* GLSL rather than a hand-written twin, deliberately — a twin would make §0's one-shader-source invariant untestable.

## Open questions worth raising

Listed at the end of `docs/REVIEW-2026-09-05.md`. Status of the three:

- **Phase 0 estimate** — resolved. Moved to 3 weeks, the number the review called honest.
- **§10.1 clipping warnings** — still live, and Spike A gave it a data point: RapidRAW paints clipped pixels pure red and pure blue directly on the photograph, replacing the pixel entirely. Worth looking at before deciding, since it is the maximal version of the thing §10.1 objects to elsewhere.
- **`providers/` as a Cargo workspace member** — the workspace now exists (root `Cargo.toml`, members `tests/color` and `tests/renderer`). Still unstated for `providers/`, and now cheap to settle by precedent.

One lives in the register: **how the RPM handles HEVC** (§16 #13 — it cannot require RPM Fusion, so it is `Recommends` plus runtime detection or nothing). §16 #14 and #15 are the same conversation with each other: golden-image thresholds have to clear both the ~0.9 ΔE YCbCr floor and one colour-attachment step, and both are due before the first `--bless`.

**The decode path is done** (`engine/`, 20 tests). A photograph becomes linear P3 f16: format sniffed from bytes, ICC read and classified, untagged assumed sRGB per §4, §7.1's proxy resampled in linear light. Three things worth knowing:

- **Spike B's harness now tests the product.** The spaces, curves, matrices and gamut policy moved from `tests/color/` into `engine/` — before that the suite validated its *own* copy, which could have been green while the shipped transforms were wrong, there being none.
- **ICC is parsed in-tree, not by lcms2** (frozen), and cross-checked against it at 7.4e-9. The trap is that ICC colorants are in the D50 connection space — comparing the tag against a D65 matrix rejects every photograph the app exists to open, so Bradford runs first.
- **Adobe RGB is nearer to Display P3 than sRGB is** (0.0901 vs 0.0934). So classification is on the whole colorant matrix, not a threshold on the red one — the obvious classifier reads Adobe RGB as Display P3, silently.

The product's decoder reproduces §3's YCbCr floor at max ΔE 0.9048 against the 0.9041 the harness measured independently, which is the number §12.1's HEIC thresholds have to clear.

**Next in v0.1:** the render path — walking a compiled graph, dispatching the fused §5 stages 2–9 pass, and getting a rendered frame out. Spike C's `adjust.wgsl` and `encode.wgsl` are the shaders; what does not exist is the thing that binds a graph node to a draw. Then export, then the panels. §16 #17 (PNG, and therefore screenshots) is small and due before v0.1 ships; §16 #16 (parameter ranges) before v0.2's presets.

`libheif-rs` is pinned to **2.7**, not 3.x: 3.x requires libheif ≥ 1.23 and Fedora ships 1.21.2. That pin is load-bearing — the binding tracks upstream closely and Fedora will lag it.
