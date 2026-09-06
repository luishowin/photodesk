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

---

## 2026-09-05 — Spike B green; spec v0.5 → v0.6

**The first code in the repository, and the first item frozen by measurement rather than by argument.** Harness at `tests/color/`, eight tests, all green. Deliverable: `SPIKE-B.md`.

### Resolved

**`Working space = linear Display P3 f16` → FROZEN.**

At thirty render passes — six layers' worth, the deepest chain §7.3 permits — f16 storage diverges from the identical chain in f64 by **ΔE2000 0.0956 max, 0.0323 mean**. §2.2's tightest threshold and §12.1's golden-image budget are both ΔE 1.0, so f16 spends under a tenth of the correctness budget at the worst depth available to it.

The four §2.2 tests: round-trip identity max 0.0000 against a 1.0 threshold (n=153); P3 → sRGB against an independent reference mean 0.0807 against 1.5 (n=126); untagged-assumed-sRGB max 0.0000 against 1.0; deep-shadow ramp bit-exact, 17 of 17 distinct output codes against a required 14, all second differences zero against a permitted 1.

**§2.2's f32 fallback is not needed and should not be built.** The reason generalises and is worth keeping: code 1/255 decodes to linear 3.03 × 10⁻⁴, where an f16 ulp is about 2.4 × 10⁻⁷ — roughly 1,270 representable values between adjacent 8-bit codes at the very bottom. Linear encoding does spend precision in the highlights as §2.2 warned, but half float's significand is *relative* and 8-bit source material never approaches exhausting it.

Error accumulates as a √n random walk, not a ratchet: ColorChecker rises 4.2× over thirty passes against √30 ≈ 5.5, and the shadow ramp is *lower* at thirty passes than at five because the alternating gain walks small values back onto exact f16 grid points.

### Newly provisional

Two items that did not exist and needed to, rather than being left as unnamed gaps.

**`ICC extraction from real containers`** (exit: the two blocked §2.2 corpus items, once `libheif-devel` is installed). Two of six corpus items — an iPhone HEIF with an embedded P3 profile, and an iPhone HEIC with an ISO gain map — need a dev package that is absent. Neither blocks the working space: synthetic corpora with exact known values are the right instrument for asking whether linear P3 at f16 holds up numerically, and that is what they answered. What the HEIF items test is whether we correctly read the tag that *selects* a transform, which is a different question and now has its own entry.

**`Export gamut-mapping policy`** (exit: before v0.1 exports). §4 said "linear P3 → tone encode → sRGB" and stopped. A P3 source exported to sRGB produces negative channels for everything outside the smaller gamut, and §2.2's test 2 cannot be written without deciding what happens to them. The harness uses clip-per-channel in linear light and agrees with lcms2 at relative colorimetric to mean ΔE 0.0807 — defensible, but a choice currently made in a test file rather than in the specification.

### Measured, not decided

**Profile misinterpretation is a quiet failure, not a loud one.** Test 3 carries a counterexample, because "assume sRGB" passing at ΔE 0.0000 says nothing unless getting it wrong is detectable. On saturated content, misreading sRGB as Display P3 costs max ΔE 4.52 — detectable. On the flat, largely desaturated content a **screenshot** actually contains, it costs max 2.96 and mean 0.50. So the failure is smallest on exactly the material most likely to arrive untagged. It is not something anyone notices by looking; it is found by a test or not at all. Spike A established that RapidRAW does this to every P3 file it opens.

### Two corrections during the spike

Both were green-looking measurements of nothing, and both would have put a false sentence in this file.

**The pass sweep measured no-ops.** The first version ran `passes` *identity* stages and reported a flat line at 1, 5 and 30 passes. Arithmetically correct and entirely uninformative: `f16 → f32 → f16` is idempotent, so an identity pass is a no-op and thirty of them are thirty no-ops. It would have supported "f16 survives a thirty-pass chain" on no evidence at all. Fixed by giving each pass real per-pixel work of the shape §5 stages 2–9 have and measuring divergence from the same workload in f64. The control remains in the suite: it prints the flat line and says why it is flat.

**The headroom table measured after 8-bit quantisation**, so every cell read 0.0000 for both f16 and f32 — the quantiser destroys exactly the quantity being reported. Fixed by measuring in the encoded float domain. The four §2.2 tests still measure at 8 bits, because their thresholds are stated on the delivered image.

### Spec changes

§4 loses its "provisional, pending Spike B" heading and gains the frozen working space plus an explicit note that the gamut policy is *not* frozen. §12 gains §12.0, recording the colour suite as permanent per §13 rather than as a spike artefact — eight tests, ~20 ms, no fixtures, so it belongs on every commit. §16 closes decision 2 and opens 11 and 12.

### Still open

**Spike C, and it now carries a warning §2.3 already wrote.** Nothing in Spike B touched a GPU; these are CPU models of the transforms. Spike C's `EXT_color_buffer_float` clause is what establishes RGBA16F as a colour-renderable target with linear filtering on this machine, and §2.3 says plainly that B and C can each be green and jointly wrong if that goes unchecked. **It is unchecked.** Freezing the working space on Spike B's evidence does not retire that risk; it concentrates it.

---

## 2026-09-05 — Spike C green; Phase 0 complete; spec v0.6 → v0.7

**All three spikes have run.** Two of the three answers were not the expected ones. Harness at `tests/renderer/`, six Rust tests plus a WebKitGTK probe. Deliverable: `SPIKE-C.md`.

### Resolved

**`Preview renderer path` → FROZEN: WebGL2 in the webview, WGSL authored fragment-first, transpiled to GLSL ES 3.00 by `naga` at build time, wgpu natively for export.**

This is §7.2's *original* candidate, not its fallback — the outcome the v0.5 entry called a hypothesis and told the spike to test rather than assume. Four things had to hold and all four do: a realistic fused pass lowers to 9,292 bytes of ES 3.00; WebKitGTK compiles and links it with no GL error; `EXT_color_buffer_float` is present with RGBA16F complete as a colour attachment and linear filtering measured at the midpoint; and six layers at 2 MP cost **5.92 ms median, 6.08 ms p95** against §7.3's 16 ms — about 1 ms per layer, scaling linearly, leaving ~10 ms for the spatial stages that pass does not yet include.

**`Shaders are authored fragment-first` → FROZEN.** `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns. This is now an invariant rather than a style: the negative control fed naga a compute shader of exactly RapidRAW's shape and it refused, naming `BUFFER_STORAGE | COMPUTE_SHADER | IMAGE_LOAD_STORE` — the three constructs §2.3 identified as having no GLSL ES 3.0 target, returned by the tool instead of reasoned about. One compute shader anywhere breaks the preview path for the whole project.

### Measured — §0's invariant, for the first time

"One shader source, preview and export" has been frozen since v0.1 of this spec on an argument. It now has a number.

`tests/renderer/tests/agreement.rs` renders the **same WGSL** through wgpu natively (Vulkan, RADV RENOIR, headless) and the probe renders the transpiled GLSL over the same source with the same parameters:

```
196,608 channel samples      max |diff| 0.008789      mean 0.0001944
over 1/255: 3 samples        orientation: direct (flipped scores 1.122)
```

Both sides store RGBA16F so the comparison is about maths rather than precision; both sample NEAREST so filtering cannot be mistaken for arithmetic. **The parameter block is not shared as bytes** — the native side writes a `#[repr(C)]` struct, the browser places named fields at std140 offsets it queries from the linked program — so agreement is evidence the two layouts match rather than evidence they read one buffer. The flipped score of 1.122 is the control that the metric is sensitive to a one-row misalignment at all.

§12.2 inherits this comparison.

**§2.3's uniform-buffer argument also holds with room to spare:** the per-layer block is **784 bytes** against a queried 65,536 and a GLES3-guaranteed 16,384. §5's per-layer loop is indeed what makes a UBO sufficient where RapidRAW's 32-slot array needed a storage buffer.

### Risk retired

The v0.6 entry closed by saying that freezing f16 on Spike B's CPU-only evidence *concentrated* a risk rather than retiring it, because §2.3's `EXT_color_buffer_float` clause was unchecked and B and C could each be green and jointly wrong. It was checked first, and it holds. **Spike B's freeze is safe.**

The filtering half of that clause is measured rather than inferred, and the distinction mattered: `OES_texture_half_float_linear` reports `false` in WebGL2 because RGBA16F filtering is core there, so reading the extension string would have produced a false failure. Sampling a 2×1 texture of 0 and 1 exactly between the texels returns 0.5.

### Two corrections during the spike

Both produced confident numbers that measured something other than what they claimed, which is now three spikes in a row where that has happened and is worth naming as a pattern rather than an accident.

**A stray debug line raised `GL_INVALID_VALUE` on every run** — `getUniformIndices` for a non-existent uniform returns `INVALID_INDEX`, and `getActiveUniform` on that raises `0x501`. The first probe reported "within budget" at every layer count with an error outstanding, which could equally have meant draws were being dropped.

**The first agreement run reported max 1.13 with 196,027 of 196,608 samples out of tolerance** — an apparently catastrophic disagreement that was entirely an input mismatch. naga names a block member three ways depending on type (`.exposure`, `.luma_curve[0]`, `.grade_shadows.offset`) and the lookup matched only the first, so the curves, the HSL bands and all four grading wheels resolved to nothing and the browser rendered zeros where the reference had values. A missing field is now a hard failure that refuses to produce a number, which is the real fix: the original failure mode was not a wrong answer, it was a wrong answer that looked like a finding.

### Re-pointed, and this time out of the spikes

**`Front-end framework` stays PROVISIONAL; its exit moves from Spike C to the author, before v0.1 scaffolding.**

Spike C was given this because it is the first thing that puts pixels in a webview. It has an answer, and the answer is that the renderer does not care: the canvas path is a WebGL2 context, a uniform buffer, three draw calls and a `requestAnimationFrame` loop, touching no framework API. `web/preview.html` runs the full stack at 2 MP as plain ES modules with no build step at all.

So the constraint the spike was meant to discover does not exist. What remains is §10/§11 UI ergonomics, which is not a thing a spike decides — and this is the second time this item has had its exit re-pointed, which is itself the signal that it was never a technical question.

### Phase 0

Estimated at 3 weeks. Actual: one day. That is not a claim that the estimate was wrong — the spikes were sequenced so each one's answer narrowed the next, and Spike A's failure removed a fork integration that was the bulk of the estimated work. What the three weeks bought was the *option* to spend them; what they cost was a day, because the answers turned out to be cheap to obtain and expensive only to guess.

Two of the three answers were not the expected ones. A was expected to pass and failed. C was expected to fall back to hand-written GLSL and did not. Only B came out where the spec predicted, and it is the one the spec was least sure about.

### Still open before v0.1

Four items, none of them a spike: the front-end framework; the export gamut-mapping policy §4 never named; ICC extraction from real containers, now unblocked since `libheif-devel` is installed; and confirmation that webkit2gtk-4.1 — Tauri's binding, as opposed to the webkitgtk-6.0 this probe ran in — behaves identically. Same engine, same WebKit 2.52.5, shared WebGL implementation, but unconfirmed.

---

## 2026-09-06 — front end decided; spec v0.7 → v0.8

**`Front-end framework` → FROZEN: none. TypeScript and Vite, zero runtime dependencies.**

The last item Phase 0 left open, and the only one it could not answer itself.

Spike C was given this because it is the first thing that puts pixels in a webview. What it returned was not a choice but the absence of a constraint: the canvas path is a WebGL2 context, a uniform buffer, three draw calls and a `requestAnimationFrame` loop, touching no framework API and identical under React, Svelte, Solid or nothing. `tests/renderer/web/preview.html` runs the whole stack at 2 MP as plain ES modules with no build step at all.

So the decision came down to §10 and §11 rather than to rendering, and there it goes the same way. §11 specifies sliders down to `Shift`-drag being 0.1× travel, `Ctrl`-drag 10×, double-click-to-reset, click-the-value for numeric entry, scroll only when the control is hovered and never when the panel is, and one completed gesture being exactly one undo entry. No component library's slider does that. It would be overridden rather than used, and overriding a control is more work than writing one.

**The cost is real and is accepted knowingly**, which is what §0 requires of a frozen item. Panels, undo, focus management and the keymap are all hand-written. None of that bill falls due at v0.1 — which is a HEIF, six sliders and an export — and all of it arrives across v0.2 to v0.7. The reason to take it anyway: this is an application of one screen with roughly forty controls, for an audience of one, that wants to still build in a decade. Framework churn is the larger of the two risks over that horizon.

Recorded in §10.3 alongside the type and colour tokens, because it is a UI decision and belongs where the rest of the UI decisions are, not in a build-tooling note.

**Phase 0 is now fully closed.** Every question §2 raised has an answer, and every item it opened has either been frozen or has a named exit.

### What remains before v0.1

Three items, all in the §0 register, none of them a spike:

- **`Export gamut-mapping policy`** — §4 says "linear P3 → tone encode → sRGB" and stops. The Spike B harness clips per channel in linear light and agrees with lcms2 at relative colorimetric to mean ΔE 0.08. Defensible, and currently decided in a test file rather than in the specification. Due before v0.1 exports anything.
- **`ICC extraction from real containers`** — `libheif-devel` is installed now, so the two blocked §2.2 corpus items are buildable. They are the only tests that would catch reading the *wrong* profile rather than applying the right one incorrectly.
- **webkit2gtk-4.1 confirmation** — Spike C measured in webkitgtk-6.0 via Epiphany; Tauri v2 binds 4.1. Same WebKit 2.52.5, shared WebGL implementation, unconfirmed. Belongs to the first v0.1 build rather than to a spike.

---

## 2026-09-06 — first real container; a platform blocker found; spec v0.8 → v0.9

`libheif-devel` was installed, which unblocked §2.2's outstanding corpus items. Building them answered one register question and opened a larger one.

### Resolved

**`ICC extraction from real containers` → FROZEN.** `tests/color/tests/heif_icc.rs`.

The container is built by the test rather than committed as a fixture — a binary is opaque, needs git-lfs (§12.1), and cannot say what it contains. lcms2 writes a 584-byte Display P3 ICC; the test attaches it to a HEIF image of the 24 ColorChecker patches re-encoded as P3, writes the container, then reopens it as an opaque file.

```
recovered ICC ............ 584 bytes, byte-identical, type 'prof'
red colorant XYZ ......... (0.5151, 0.2412, -0.0011)      P3, not sRGB's 0.4361
P3-tagged -> sRGB ........ max ΔE 0.4055   mean 0.0423
same file, profile ignored max ΔE 3.4332   mean 1.8369
```

Three assertions rather than one, because a container can round-trip bytes while the app still fails to act on them: the ICC comes back identical, it *parses* and describes P3's primaries rather than sRGB's, and driving the transform from it lands inside ΔE 1.5. The fourth number is the counterexample — ignoring the profile costs 3.43, comfortably above the threshold the test applies, so a green result distinguishes reading the tag from ignoring it. That is the exact failure Spike A found RapidRAW committing on every P3 file it opens.

**Encoded `uncompressed`, deliberately.** Any codec loss would land in the ΔE and be indistinguishable from a profile error, which is the one thing this test must not confuse.

### The larger finding

**`HEVC decode needs libheif-freeworld` → FROZEN as a platform fact.**

Enumerating libheif's codecs rather than assuming them (`tests/color/tests/heif_codecs.rs`) returned:

```
HEVC (iPhone HEIC)     decoders: —                        encoders: —
AV1 (AVIF)             dav1d v7.0.0, libaom v3.13.3       libaom, SVT-AV1, rav1e
AVC (H.264)            OpenH264 2.6.0                     —
JPEG                   libjpeg-turbo 3.1.2                libjpeg-turbo
JPEG 2000              OpenJPEG 2.5.4                     OpenJPEG
uncompressed           builtin                            builtin
```

**Fedora's libheif has no HEVC codec at all**, in either direction. It is a licensing decision rather than an oversight — HEVC is patent-encumbered and Fedora will not ship it — and RPM Fusion's `libheif-freeworld` supplies it. Confirmed at the library level: `libheif.so.1` links libaom, SVT-AV1, libjpeg and openh264, and neither libde265 nor x265, with no plugin directory.

An iPhone HEIC is HEVC. So **§1's native subject and the whole of §14's v0.1 do not open on a stock Fedora install.**

This is exactly the class of thing §2 exists to surface early: cheap to find now, and a week of confused debugging in month two. It cost one enumeration.

### Consequences written into the spec

§3 gains the fact, next to where formats are discussed. §13 gains the part that actually bites: **PhotoDesk's RPM cannot satisfy its own most important dependency**, because a Fedora package may not require a third-party repository. Three options are recorded there — a hard `Requires` that refuses to install, a `Recommends` plus runtime detection, or bundling an encumbered codec. The second matches §9.4's existing posture, where a missing capability greys out with a reason and nothing else changes, and it is the only one where the application is still useful on a stock install. Opened as §16 #13, due before v0.7 packaging — but noted now, because **v0.1's decode path has to return a distinguishable "no codec" error rather than a generic failure**, and that is cheaper to build in than to retrofit.

### Still blocked

**The HDR gain-map corpus item.** It needs both the HEVC codec and a real iPhone HEIC; neither is present. `v1 discards iPhone HDR gain maps` stays PROVISIONAL with its existing exit (§4) — the register entry was always about the *decision*, and what remains missing is the test that the SDR base decodes correctly while the gain map is ignored rather than misapplied.

### On the dependency

`libheif-rs` 3.x requires libheif ≥ 1.23; Fedora ships 1.21.2. Pinned to `libheif-rs` 2.7, which builds and runs against it. Worth knowing that this binding tracks upstream libheif closely and Fedora will lag it, so the pin is load-bearing rather than incidental.

---

## 2026-09-06 — HEVC installed; the iPhone HEIC path tested; spec v0.9 → v0.10

`libheif-freeworld` is installed. HEVC now enumerates as `libde265 1.0.18` and `FFMPEG AVC/HEVC 8.1.2` for decode, `x265 4.1` for encode, loaded as plugins from `/usr/lib64/libheif/` — the library itself still links neither, which is why the earlier `ldd` reading was correct and the codec list is the thing worth trusting.

### Resolved

**The iPhone HEIC path is tested.** `heif_icc.rs` now runs its whole case over **every container this machine can write** rather than the first one it finds — uncompressed, AVIF and HEVC. All three pass. The ICC comes back byte-identical from all three, parses to P3's red primary in all three, and the profile-honoured result stays inside ΔE 1.5 while the profile-ignored counterexample sits at 3.43.

**The codec probe now asserts rather than reports.** A machine without HEVC cannot open §1's native subject, and nothing else in the application would say so clearly — an iPhone HEIC would simply fail. A red test is the cheapest possible way to say "this machine is not provisioned".

### Measured, and it sets a floor

**`HEIC sources carry a ~0.9 ΔE conversion floor` → FROZEN as a format fact.**

```
uncompressed   codec loss  max 0.0000  mean 0.0000
AV1 (AVIF)     codec loss  max 0.9041  mean 0.2920
HEVC (HEIC)    codec loss  max 0.9041  mean 0.2920
```

Two different codecs, two different implementations — libaom and x265 — both asked for lossless, returning **the same number to four decimal places**. That is not compression. It is the RGB↔YCbCr↔RGB conversion libheif performs around every YCbCr codec, and only the uncompressed container escapes it.

Apple does not ship uncompressed. **So every HEIC PhotoDesk opens has already spent ΔE 0.9 before §4 sees a pixel**, and no care taken later buys it back. Recorded in §3 next to the codec fact, and opened as §16 #14: **§12.1's golden-image thresholds for HEIC sources have to clear this floor**, and that has to be settled before the first `--bless` rather than discovered as a suite that will not go green.

Worth noticing what found this. The test measured codec loss separately from profile error instead of asserting exact pixels — which the earlier uncompressed-only version did, and which would have been a correct assertion for uncompressed and a wrong one for the container the product actually cares about. Separating the two turned a threshold that would have had to be loosened into a fact with a cause.

### Still blocked

**The HDR gain-map corpus item**, and now for only one reason. The codec is present; what is missing is a real iPhone HEIC — there are none on this machine. `v1 discards iPhone HDR gain maps` keeps its existing exit (§4); what remains untested is that the SDR base decodes correctly while the gain map is ignored rather than misapplied. One photograph off a modern iPhone closes it.

---

## 2026-09-06 — real photographs; §4 verified against the device; spec v0.10 → v0.11

An iPhone HEIC and an iPhone JPEG were made available. §2.2's last blocked corpus item is now exercised, and §4 has been checked against the thing it was written about rather than against a description of it.

**The photographs are not in the repository and must not be.** They are personal files, §12.1 puts corpus binaries behind git-lfs, and `tests/color/tests/real_photos.rs` reads from `PHOTODESK_CORPUS_DIR` (defaulting to `~/Downloads`) and skips with an explanation when the directory is empty. The test reports structure and colour only — EXIF blocks are listed by type and size, never by value, because §6.1's export default is `keep-minus-gps` and a test log is not the place to leak a location.

### §4's assumptions, all of which held

| | HEIC | JPEG |
|---|---|---|
| Colour tag | ICC, 536 bytes | ICC, 536 bytes, reassembled from APP2 chunks |
| Red colorant XYZ | (0.5151, 0.2412, −0.0011) | identical |
| Interpretation | Display P3 | Display P3 |
| Base image | 3024 × 4032, 8-bit luma and chroma | — |
| Round trip through linear P3 f16 | **max ΔE 0.0000**, n=3072 | — |

Three assumptions became facts. The manufacturer really does tag Display P3, in both containers, with the same profile — so §1's "take the manufacturer's rendering as the starting point" has something concrete to read rather than a hoped-for tag. The base image is **8-bit**, which is what carries Spike B's f16 headroom argument from synthetic ramps onto real material. And a real photograph survives the working space *exactly*: 0.0000, not 0.03.

The JPEG's profile is reassembled by hand from APP2 segments rather than pulled from a decoder — thirty lines, no dependency, and the chunking is the part that goes wrong. A profile over 64 KB is split across numbered chunks that must be concatenated in order; miss that and a large profile silently truncates into something that still parses.

### The gain map, seen rather than assumed

`urn:com:apple:photo:2020:aux:hdrgainmap`, carried as an **auxiliary image** inside the same container rather than as a second top-level image, at **half resolution** — 1512 × 2016 against 3024 × 4032 — and 8-bit.

Two consequences, both recorded in §4. **libheif's default decode returns the SDR base and does not apply it**, so v1's stated behaviour is what falls out of doing nothing, which is the safe direction and not a coincidence worth relying on silently. And if the exit condition is ever met, the map needs **upsampling** to base resolution rather than one-to-one sampling.

`v1 discards iPhone HDR gain maps` keeps its exit condition and its PROVISIONAL state — the decision has not changed — but it is no longer untested. §4 insisted this be written down rather than merely implemented; it is now also a skip that can be seen.

### The number that moved

The same real pixels exported to sRGB shift by **max ΔE 2.86, mean 0.29**. That is not an error — it is P3 content being gamut-mapped, and it is visible at the top end.

So **§16 #11 is not an abstract tidiness item.** The export gamut-mapping policy §4 never named is worth up to three ΔE on the user's own photographs, and it is currently decided in a test file (`GamutPolicy::ClipLinear`) rather than in the specification. That was easy to defer while the only evidence was a synthetic sweep containing the primaries themselves; it is harder to defer now.

### Closed

§2.2's corpus is complete. Every item it listed — the synthetic chart in both spaces, a real P3-tagged HEIF, a gain-mapped HEIC, an untagged screenshot, a wide-gamut gradient, and the deep-shadow ramp — now exists and is exercised.

---

## 2026-09-06 — §16 #11 closed: the export gamut-mapping policy; spec v0.11 → v0.12

§4 said "linear P3 → tone encode → sRGB" and stopped. The policy that filled the gap was one `clamp` in `working.rs`, put there so Spike B's test 2 could be written at all, and recorded at the time as "defensible, but it *is* a choice, and it is currently made in a test harness rather than in the specification". The real photographs of the previous entry priced it: **max ΔE 2.86 on the user's own pictures**. This entry closes it.

Harness: `tests/color/src/gamut.rs`, `tests/color/tests/gamut_policy.rs`, `tests/renderer/shaders/encode.wgsl`, `tests/renderer/tests/encode_stage.rs`.

### The measurement had to be arranged around a trap

The obvious instrument is ΔE from the original, lowest wins. It picks the wrong policy, and it does so *confidently* — the fourth time in this project a clean number has turned out to be about something else.

Clamping each channel to [0,1] is exactly the Euclidean projection onto the gamut cube. **The clip is the nearest in-gamut colour**, measured in linear RGB — a space nobody perceives in. Searching for the perceptually nearest colour scores better still and is not shippable:

| policy | mean ΔE | max ΔE | fragment shader? |
|---|---|---|---|
| clip each channel | 3.3176 | 6.8254 | yes |
| **constant-luminance chroma clip** | 4.5022 | 13.5083 | yes |
| + soft knee, k = 0.95 | 4.6830 | 13.6633 | yes |
| nearest in Lab (control) | **3.0906** | **5.7889** | **no — a per-pixel search** |

Both distance-minimising policies leave **81 of 81** out-of-gamut samples *on* the gamut surface, because minimising a distance is projecting to the surface whatever the distance is. The lowest ΔE and the flattest gradient are the same answer, so ΔE cannot rank these and the file does not try.

### Nor is there an answer to defer to

Perceptual rendering lives in a profile's B2A lookup tables, and neither sRGB nor Display P3 has any — they are matrix/TRC profiles. lcms2 returns the same transform to **ΔE 0.000000** whether asked for perceptual, saturation or relative colorimetric. Asserted rather than believed, because it is the single fact that decides whether §16 #11 is our decision or somebody else's default.

### What the error is made of

| | clip each channel | **constant-luminance chroma clip** | + knee 0.95 |
|---|---|---|---|
| \|ΔL*\| max / mean | 2.815 / 1.143 | **0.000 / 0.000** | 0.000 / 0.000 |
| \|ΔC*\| max / mean | **37.97 / 13.02** | 58.05 / 18.39 | 58.85 / 19.34 |
| \|ΔH\| max / mean | **10.73 / 2.16** | 25.36 / 4.00 | 25.57 / 4.14 |
| worst boundary ramp | **28 of 33 codes**, one step at ΔE 0.0000 | 33 of 33, min step 0.2688 | 33 of 33 |
| in-gamut colour | bit-identical | bit-identical | up to ΔE 0.69 |

Reported as ΔL\*/ΔC\*/ΔH rather than as a hue *angle*: swinging a nearly-grey colour thirty degrees is arithmetic, not a visible error, and the angle overstates itself at low chroma. The chroma-weighted term confirmed the effect was real rather than an artefact of the unit — which is what it was there to decide.

### The photograph reversed what the synthetic ramp implied

The ramps are built from mid grey to the Display P3 corners and are hostile on purpose. A photograph is not. Adjacent-pixel collapse on a real iPhone HEIC (`IMG_7604.HEIC`, 3024 × 4032, Display P3; 54,338 sampled pixels, 3.06% outside sRGB; 60,304 neighbouring pairs that differ in the source and touch the boundary):

| policy | pairs merged into one colour | frame touched that the clip left alone | in-gamut max ΔE |
|---|---|---|---|
| clip each channel | 75 (0.12%) | 0.00% | 0.0000 |
| **constant-luminance chroma clip** | **39 (0.06%)** | **0.00%** | **0.0000** |
| + knee 0.95 | 29 (0.05%) | 3.23% | 1.0102 |
| + knee 0.90 | 30 | 10.41% | 2.0043 |
| + knee 0.80 | 33 | 22.23% | 3.8222 |

The flattening the ramp dramatises is, on a photograph, seventy-five pixel pairs in sixty thousand. **Compression buys ten of them by modifying 3.23% of the frame that was already correct** — a broad, measurable cost against a narrow, invisible one. Rejected.

### Frozen: clip chroma at constant luminance

Slide the colour along the ray from the achromatic point *of its own luminance* until it is exactly on the gamut boundary. The vector being scaled carries zero luminance by construction, so L\* is exact rather than nearly-exact and chroma is the only thing spent.

Three reasons, each a sentence, per §0's rule:

- **The clip's error has no policy.** How much lightness a colour loses depends on which channel happened to run out first — up to 2.8 L\* — and a WYSIWYG editor cannot say what became of a colour if the answer is "it depends".
- **It halves the detail loss for free**: 75 merged pairs to 39, at zero cost inside the gamut and the same handful of ALU ops.
- **It has no constant to tune.** The knee variant is better at gradients and was rejected for exactly that reason — a number with no derivation behind it is not frozen, it is a habit.

**The cost, recorded rather than discovered later.** On extreme saturation this policy moves hue and chroma more than the clip does, ΔH 25.4 against 10.7, on a corpus containing the P3 primaries themselves. No camera produces those colours and on the photograph the gap is 0.13 ΔE of mean movement — but it is the direction the policy is weakest in. A golden image showing a saturated red drift toward pink is the signal to reopen this, and `GamutPolicy::CompressLuma` plus its knee sweep are in the tree so that reopening it is a one-line change.

### It runs where §0 requires it to run

Stage 13 is the only stage every pixel of *both* paths goes through, and §0 freezes preview and export to one shader source — so a policy that cannot be expressed in a fragment shader is not adoptable whatever its colorimetry. That is the shape of the finding that killed the fork, and this time it was asked in advance rather than discovered.

`shaders/encode.wgsl` lowers to GLSL ES 3.00 (1,385 bytes vertex, 2,534 fragment) and, run through wgpu, agrees with the Rust reference to **one colour-attachment step** — bit-exactly the reference truncated to f16 on **48,020 of 49,152 channels**. The reference is imported from `photodesk-color` rather than transcribed, so the two sides cannot agree with each other and be wrong together.

**New register item, and it is not about this policy.** Getting that agreement to read cleanly meant finding out why it was 4.9 × 10⁻⁴ off, and the answer was the instrument again: on RADV/RENOIR the **RGBA16F colour attachment truncates toward zero rather than rounding to nearest**. A full-step bias on every stored channel, the driver's rounding mode and not the shader's arithmetic. Recorded as a platform fact and opened as §16 #15, because a golden-image threshold that does not allow for it will fail a correct render.

### One green test was measuring the wrong thing, and said so

Flipping the frozen constant took Spike B's **test 2 from mean ΔE 0.0807 to 1.4183 against its own 1.5 threshold** — still green, and no longer measuring what it was written to measure. Its reference converter clamps in linear f64, so it *is* clip-in-linear; running the pipeline under any other policy turns the test into a comparison between two gamut policies wearing a transform-fidelity label.

Test 2 is now pinned to `GamutPolicy::ClipLinear` with the reason written next to it. This is the same failure mode as the pass sweep of no-ops and the headroom table destroyed by quantisation, and it is worth noting that the thing that caught it was a number moving by seventeen times while staying inside its bar.

### Also in this change

**§2.2's `BLOCKED` corpus list is empty and kept.** It still declared two items as needing `libheif-devel`, which arrived on 2026-09-06 — the previous entry closed both, and the code had not heard. The mechanism stays, and empty, because the next blocked item should announce itself in a test run.

**The corpus photographs were removed from `~/Downloads` during this session.** The real-photograph figures above are from `IMG_7604.HEIC` and are not reproducible on this machine until a photograph is put back; `real_photos.rs` and `gamut_policy.rs` both skip with an explanation, which is the arrangement those tests were built for. The ΔL\*/ΔC\*/ΔH decomposition on real pixels was not captured before the directory emptied — the test computes it now, and it is the one number in this entry that is still owed.

The path was exercised after the fact against a **synthetic P3 gradient stand-in** built with libheif — a 1200 × 1600 saturated sweep, 85% of it outside sRGB, which is a corpus item and emphatically not a photograph. Reported only because the shape it gives is the same one at a larger amplitude: collapse falls from **15,556 of 128,007 pairs under the clip to 4,673** under the frozen policy, and the knee then recovers only **267** more. On saturated content the constant-luminance clip takes nearly all of the available gradient back, and the knee is buying the last few percent at full price.

---

## 2026-09-06 — the preview path confirmed in Tauri's own webview; spec v0.12 → v0.13

`SPIKE-C.md` ends with a caveat rather than a result: *"The engine is right; the binding is not identical. This ran in **webkitgtk-6.0** via Epiphany 50. Tauri v2 on Linux uses **webkit2gtk-4.1**. Both are installed here, both are WebKit 2.52.5, and they share the WebGL implementation — but this has not been confirmed inside Tauri's own webview, and that confirmation belongs to the first v0.1 build."*

It did not have to wait for a build. There is no browser that ships the 4.1 binding — Epiphany moved to 6.0 — but the binding can be driven directly: forty lines of PyGObject open a GTK 3 window with a `WebKit2.WebView` in it and point it at the same probe. `run-probe.py` now takes `--engine epiphany | webkit2gtk-4.1` so the two produce the same report in the same format, which is what makes them comparable rather than merely both green.

Both runs below are the same machine on the same day, so the driver, the shaders and the generated GLSL are identical inputs.

| | webkitgtk-6.0 (Epiphany 50) | **webkit2gtk-4.1** (embedded WebView) |
|---|---|---|
| WebKit | 2.52.5 | 2.52.5 |
| `EXT_color_buffer_float` | present | present |
| RGBA16F colour attachment | `COMPLETE` | `COMPLETE` |
| RGBA16F linear filtering, *measured* | 0.5 at the midpoint | 0.5 at the midpoint |
| transpiled GLSL | compiles and links, 4,683 / 9,292 B | compiles and links, 4,683 / 9,292 B |
| uniform block | 784 of 65,536 bytes | 784 of 65,536 bytes |
| six layers at 2 MP, median / p95 | 5.92 / 6.17 ms | **6.17 / 6.42 ms** |
| WebGL2 vs wgpu | max 0.008789, mean 0.0001944, 3 channels over 1/255 | **max 0.008789, mean 0.0001944, 3 channels over 1/255** |
| GL error at end | none | none |

**The agreement figures match to every digit printed**, which is the part worth trusting: two bindings that ran the same GL code on the same driver produce the same pixels, and no amount of "both are WebKit 2.52.5" would have established that on its own.

The one difference is timing — 6.17 ms against 5.92 at the §7.3 bound, about 4%, against a 16 ms budget. Small enough to be the GTK 3 compositing path or run-to-run variance, and not worth attributing without a reason to care. Both are inside budget at eight layers, two past the bound §7.3 sets.

Worth noting in passing: the 6.0 run reproduced `SPIKE-C.md`'s 5.92 ms exactly, a day and a spec version later. That is a check on the harness, not on the engine.

### What moves

Nothing between states. `Preview renderer path: WebGL2 + WGSL→GLSL via naga` was already FROZEN; what this retires is the **residual risk attached to it**, and the register reason now says where it was re-run. The last item on the "before v0.1" list that was not a spike is done — the remaining work is v0.1 itself.

`SPIKE-C.md` is not edited. It records what was measured on 2026-09-05 and its caveat was correct on the day.

---

## 2026-09-06 — v0.1 begins: the document model; spec v0.13 → v0.14

The first product code in the repository. §14 gives v0.1 "document model, graph compile, source-preservation and golden tests running" alongside the six sliders, and puts the reason plainly: the boring parts are the ones that are expensive to retrofit and impossible to bolt on later. This is the first of them.

`src-tauri/src/photodesk/{document,sidecar}`, 36 tests.

### Two decisions §0 required before any of it could be written

**The document schema's home is Rust, and the TypeScript types are generated from it. FROZEN.**

§13 puts `src/document/` in the front end, which invites the reading that the schema is a TypeScript concern and Rust receives something already parsed. That reading fails on §12.3. Source preservation is *"for each source in corpus: hash, open → apply document → render → export, assert the hash is unchanged"* — a headless Rust test, on every commit, that needs a document. So does §12.1's golden-image corpus. A schema reachable only through a webview cannot serve either, and "the processing chain can be invoked headlessly from a test binary, with no webview and no front-end state" is **criterion 1 of the gate Spike A failed RapidRAW on**. Adopting the same shape in our own tree, having rejected a fork over it, would be the worst kind of consistency.

The other direction — a Rust schema plus a hand-written TypeScript one — is two schemas, and the one that drifts is the one with no test looking at it. The project already has an answer to that shape and it is in the tree: `tests/renderer/web/` loads **naga-generated GLSL** rather than a hand-written twin, deliberately, because a twin would make §0's one-shader-source invariant untestable. Same argument, different artefact. The types are generated into `src/document/generated/`, **committed** so a fresh clone builds, and a test rewrites the file and then fails if the contents moved — so the fix is already applied by the time anyone reads the message.

`ts-rs` is a dev-dependency and the derives are `#[cfg_attr(test, ...)]`, so nothing generated reaches a shipped binary. `cargo build --workspace` is warning-free; the one warning in a test build is ts-rs declining to parse `deny_unknown_fields`, which it ignores and which TypeScript expresses structurally anyway.

**`src-tauri/` links no webview yet.** Tauri's four devel packages are absent on this machine, but that is not the reason — the reason is the paragraph above. `tauri` and a `main.rs` arrive when there is a window to open. It is also how a Tauri v2 project is laid out anyway.

### A test found a bug in §6.1, in the workflow §14 gives v0.1

§6.1 shows the sidecar as `IMG_4821.HEIC` beside `IMG_4821.photodesk.json` — extension dropped, which is tidier. The naming test asserted that two photographs with the same stem get one sidecar each, the implementation truncated at the last dot, and the two disagreed.

The implementation was right and the spec was wrong, and the case is not a corner: **v0.1 is "open a HEIF → … → export"**, so an export beside its source is `IMG_4821.jpg` next to `IMG_4821.HEIC`. Under §6.1's naming those two share one sidecar, and editing the export overwrites the original's edits with no error and nothing to notice. Data loss, in the one workflow the release exists to deliver.

**Frozen: the sidecar keeps the whole filename** — `IMG_4821.HEIC.photodesk.json`. It removes the collision rather than detecting it, at the cost of a longer name that still sorts beside its photograph. darktable settled the same question the same way; Lightroom did not, and the collision between a raw and its JPEG is a known complaint about it. §6.1 is corrected.

### §6.3's table is six rows, and three of them are not accept-or-reject

| Condition | Behaviour | Where it is |
|---|---|---|
| `photodesk` newer | Reject, clear message | `MigrateError::FromTheFuture` |
| older, migration exists | Migrate on load, write back on next save | `Notice::Migrated` |
| older, no migration | Open **read-only**, offer export-as-new | `ReadOnly::NoMigrationPath` |
| `pipeline_version` older | Open, warn, **never silently re-render** | `Notice::PipelineIsOlder` |
| unknown `op` / `op_version` | Reject | `Op` is a closed enum; `ParamsError::UnknownOpVersion` |
| unknown key in `params` | Validation error | `deny_unknown_fields`, and the message names the key |

A table like that gets implemented for the two easy rows and remembered for the others, so it is in the types rather than in a comment. Loading returns a `Loaded`, not a `Result<Document, _>`: the document is **not a public field**, `into_writable()` is the only door to an owned one and it fails with §6.3's reason, and `save` takes an owned document — so **read-only is enforced by the compiler**. Everything the user has to be told is a `Vec<Notice>` the caller has to look at rather than a flag it can forget.

The migration registry is **empty and correct** — schema 1 is the first. The machinery exists because §6.3 gives five *other* conditions defined behaviour, and writing those at the moment the first migration lands means writing them under pressure with a user's edits in the balance. `migrate` takes its registry as a parameter, so composition is tested against a synthetic chain rather than by putting a fictional step in the shipped registry to make a test pass. Three properties are asserted: steps run in order and stop at the target, a gap is an error rather than a silent stop, and **a failed step leaves the document untouched** — a half-migrated document is worse than an unmigrated one because it looks readable.

### The two §6.1 rules that shaped the types

**`params` is a fixed schema per `op` + `op_version`.** Nothing serde offers dispatches on that: the discriminant is a *pair of sibling fields* next to the object rather than a tag inside it. So `Layer` has a hand-written `Deserialize` — forty lines — that reads the pair first and types the params second. The alternative, storing params as untyped JSON and validating later, would mean a `Document` in memory could be invalid, and "a Document has been validated" is worth more than the forty lines.

**Omitted keys mean identity, not zero**, so every parameter is `Option` even where zero is the identity value. That is load-bearing rather than tidy, and the test proves it by doing the operation it exists for: §6.1 makes a tool preset something that *merges into* the stack, so a preset saying nothing about exposure must leave exposure alone. With a plain `f32`, "sets exposure to 0" and "says nothing about exposure" are the same value and one of them silently discards the user's work.

`adjust` v1 carries the nine scalars of §5's stages 2–5 and 9, where §14 gives v0.1 six sliders. The extra three belong to the same op and adding them later would cost an `op_version` bump to buy nothing. Stages 6–8 are absent: a curve, an HSL band set and a grading wheel are structured rather than scalar, they arrive at v0.3, and they will come with the version bump they actually justify.

### §0's first frozen item, tested at the scope that exists

§12.3 is the whole cycle and two of its five steps do not exist yet. What exists is the part that *writes*, which is the part that could break the invariant, so it is asserted now rather than when the render path arrives and there are three suspects instead of one: create, save, reopen, edit, save again, re-hash the photograph. Byte-identical **and** the same mtime, because a rewrite with identical bytes is still a rewrite and it is the kind that survives a hash comparison. With the counterexample, so a hash that cannot see a change is not mistaken for a test that passed.

Sidecar writes are atomic — temp file beside the target, then rename — because a half-written sidecar is worse than an absent one: it looks like a corrupt document rather than a missing one, and §6.3 has no row for "truncated". And the serialisation is canonical, so **opening a photograph and saving it back produces the same bytes**: a loader that materialised defaults or normalised anything would mean merely looking at a photo dirties its sidecar, and a user with a backup tool sees churn they did not cause.

### Opened

**§16 #16 — parameter ranges.** The document validates finiteness and no bounds, so `exposure: 400` is a legal document. Deliberate: §6.1 states no ranges and §11 puts slider travel in the UI, so inventing numbers here would put them in the register's blind spot. Due before v0.2's presets, which are the first thing that writes params the UI did not.

---

## 2026-09-06 — §6.2's graph compile; spec v0.14 → v0.15

`src-tauri/src/photodesk/graph/`, 18 tests. The document is stack-shaped on disk and the UI is always a stack; internally it now compiles to the typed DAG §6.2 describes.

### The decision it forced

**The graph is compiled once, in Rust, and both renderers execute the same plan. FROZEN.**

§13 puts "DAG compile, dirty tracking" in `src/graph/`, the TypeScript front end. That cannot be right, and the reason is one §0 has already frozen a whole invariant over. The preview runs in the webview and the export runs natively through wgpu (§7.2). **If each compiled its own graph, the preview would render one topology and the export another** — and they would drift exactly as two shader sources would, with the same property that nobody notices until an exported file differs from what was on screen. §12.2 would then be comparing two *compilations* rather than two executions of one plan, and the one-shader-source invariant it exists to enforce would be enforced over a shader while the graph above it went unchecked.

So the compile is here, the plan is serialised, and `src/graph/` executes it. The TypeScript types are generated from the same declarations as the document's. §13 is corrected; this is the third time the "one artefact, generated, not two hand-written ones" argument has decided something in this project, and it has not been wrong yet.

### One mechanism, four benefits

§6.2 lists identity-node elimination, common-subexpression caching, dirty-subgraph invalidation and a stable target as benefits that arrive *for free*. Free is a claim about a design, so it is worth saying what actually delivers it: **a node's key is `H(what it does, the keys of its inputs)`**. A Merkle hash, so it covers the whole subgraph beneath it.

- **Deduplication is a lookup on insertion**, not a pass afterwards — there is never a moment when the duplicate exists.
- **The dirty set is key-set subtraction.** Recompiling after an edit gives identical keys for everything the edit did not reach.

That last one generalises further than §6.2 asks, at no extra cost. §6.2 wants invalidation "on a slider drag"; the same subtraction answers a **reorder**, an **insertion** and a **deletion** with no additional machinery. The tests cover all four, and the deletion case is the one worth reading: removing the last of three layers dirties **exactly one node**, the encode, because it now consumes a different image while both surviving adjustments keep their keys. A positional dirty flag would have invalidated everything after the deletion.

### Two rules from elsewhere, made structural

**§9.3's feather rule.** *"Feather is deliberately not in the key… putting it in `input_state_hash` would make every nudge of the feather slider invalidate the embedding and re-run the segmenter, turning a free control into a multi-second one."* Feather and invert are compiled as their own nodes downstream of the mask shape, so the shape's key **cannot** contain the radius — the mistake is not expressible rather than merely discouraged. Two layers with the same AI mask at different feathers share the segmentation and differ only in the blur; the test asserts one `mask_shape` and two `mask_feather`.

**§9.3's resolution rule, and §12.2's premise.** The node key is deliberately scale-free, because §12.2 renders one document at proxy and at full-res and asserts they match — which only means something if both runs execute one graph. Resolution belongs to execution, and to caching: `cache_key(w, h)` folds it in, which is §9.3's `mask_resolution` and its "different caches, different keys, don't share one scheme" at once. Without it, "a proxy-resolution mask silently serving a full-resolution export" is a one-line bug.

### The control

Deduplication that shared *too much* would look like an even better result, so the test that matters is the one that must not share.

§8's frozen register item says `luminance` and `color` read **the layer's input**. Two layers with an identical luminance range at different stack positions therefore select different pixels — the second sees the first layer's adjustment already applied. Sharing one node between them would be a real bug producing a plausible image, which is precisely why §8 wrote the rule down: unnamed, "it gets chosen accidentally and differently in the preview and the export".

The compiler makes it structural. A component that reads pixels takes the layer's input as a graph input and is keyed by everything upstream; one that does not takes no input at all. So the same two layers sharing a *linear gradient* compile to one node and sharing a *luminance range* compile to two, and the test asserts both halves — the second is what makes the first a consequence rather than a coincidence.

### What is eliminated, and what deliberately is not

Disabled layers, identity layers, identity geometry, and single-component masks (folding one value is that value). And §5's own sentence, which earns its own test: *"a `mask: null` layer composites at full coverage, so its mask multiply and composite are identity and both are skipped"*. Getting that wrong is invisible in the output and expensive in the budget — a composite is a full-frame texture round-trip, and §7.3 sizes the frame budget on four or five passes per layer rather than six.

**Nothing is reordered, merged across layers, or algebraically simplified.** §5 freezes that pipeline order is explicit and versioned, so an optimiser deciding two adjacent adjustments could be one pass would be changing an order no document records. Elimination removes what does nothing; it never rewrites what does something.

And compiling a document written under an older `pipeline_version` is an **error**, not a best effort. §6.3 says open and warn, never silently re-render — compiling under this build's ordering would be that re-render, and it would be invisible: the image would simply look different from the last time the user saw it, with nothing to point at.

### A serde trap worth recording

`NodeKind::MaskCompose(MaskOp)` — an internally-tagged newtype variant holding a *string* — compiles, generates plausible TypeScript, and **fails at run time** the first time a two-component mask is keyed, because serde cannot serialise that shape. It is now a struct variant. The key builder hashes `serde_json::to_vec(kind)` rather than a hand-written encoder, deliberately, so that a field added to a node kind is covered by the key automatically; the cost of that choice is that a serialisation failure becomes a panic, and this is the shape that produces one.

The binding-staleness test now covers **both** generated files. It covered one, which is a guard that lets the other rot — the failure it exists to prevent.

---

## 2026-09-06 — v0.1's decode path; spec v0.15 → v0.16

§5 stage 0: a photograph on disk becomes linear Display P3 f16 in memory. `engine/{colour,icc,decode,image,gamut}`, 20 new tests, 102 in the workspace.

### The move that had to happen first

**Spike B's harness was validating a copy.** The spaces, transfer curves, matrices and gamut policy lived in `tests/color/`, because when they were written there was no product to put them in — so `tests/color/` cross-validated *its own* transforms against lcms2. That is a weaker claim than it looks. The suite could have been green while the shipped transforms were wrong, for the simple reason that there were none.

They now live in `engine/`, and the harness imports them. What stays behind is measurement: ΔE2000, Lab, the corpus, and the pipeline model whose precision is a parameter. The split is on "does a photograph go through this?" — Lab does not, and §1's non-goals are emphatic enough about soft-proofing that shipping unused colour science invites somebody to use it. The `nearest_in_gamut_lab` control went with it, which is where a control belongs.

`tests/renderer/`'s stage-13 agreement test now checks the shader against the **shipped** gamut policy rather than a harness copy of it. That was already the intent; it is now the fact.

### ICC is parsed in-tree, and the trap is the connection space

**FROZEN: profiles are parsed here, not by lcms2.** The same argument the harness makes about matrices — derive rather than copy — applies to the code that decides how a photograph is interpreted: it should be code this project can read, and the harness should be able to disagree with it. `tests/color/tests/decode_path.rs` checks this parser against lcms2 reading the same bytes:

```
sRGB       lcms2 red colorant (D50) [0.4360, 0.2225,  0.0139]
           adapted to D65           [0.4124, 0.2126,  0.0193]   ours identical to 7.4e-9
Display P3 lcms2 red colorant (D50) [0.5151, 0.2412, -0.0011]
           adapted to D65           [0.4866, 0.2290, -0.0000]   ours identical to 7.1e-9
```

That D50 value for Display P3 — (0.5151, 0.2412, −0.0011) — is exactly what §4 recorded from a real iPhone profile, so the parser reads Apple's files the way the earlier measurement did.

**The trap is that ICC colorants are in the profile connection space, which is D50.** Display P3's red primary at D65 is (0.4866, 0.2290, 0.0000); the tag says (0.5151, 0.2412, −0.0011). Comparing the tag against a D65-derived matrix finds neither of §4's spaces and rejects every photograph the application exists to open. Bradford adaptation runs before any comparison.

### The tolerance is derived, and a threshold would have been wrong

**FROZEN: a profile that is neither of §4's two spaces is refused, not rounded to the nearest.** Classification is on the whole 3×3 colorant matrix, at a tolerance of 0.02, and both of those are consequences rather than preferences:

| pair | largest component gap |
|---|---|
| sRGB ↔ Display P3 | 0.0934 |
| **Display P3 ↔ Adobe RGB** | **0.0901** |
| sRGB ↔ Adobe RGB | 0.1720 |
| sRGB ↔ ProPhoto | 0.3854 |

**Adobe RGB is nearer to Display P3 than sRGB is.** It shares sRGB's red and blue primaries, so the obvious classifier — a threshold on the red colorant, which is what §4's own measurement note and `real_photos.rs` both use — reads Adobe RGB as Display P3. That is a silent wrong colour on a profile people actually have, and there is a test named for it. At 0.02 a profile must be less than a quarter of the way from P3 to Adobe RGB to be accepted as P3, while the tags' own quantisation (s15Fixed16, 1.5 × 10⁻⁵) sits three orders of magnitude below.

The tone curve is checked as well. Primaries are half of a space: Display P3's primaries with a 1.8 gamma is not Display P3, and both of §4's spaces use the sRGB curve — Display P3 uses it rather than DCI's 2.6 gamma, which `colour.rs` already flags as a ~4 ΔE error that looks like a gamut problem.

### The decode, measured against a container rather than a mock

The 24 ColorChecker patches encoded as Display P3, written into every container this machine can produce, then opened by the product knowing nothing about how they were made:

```
uncompressed   decode -> working space -> encode: max ΔE 0.0314  mean 0.0067
AV1 (AVIF)                                        max ΔE 0.9048  mean 0.2940
HEVC (HEIC)                                       max ΔE 0.9048  mean 0.2940
```

**That reproduces §3's YCbCr conversion floor** — measured independently at 0.9041 max, 0.2920 mean — through the product's decoder this time rather than the harness's, and the uncompressed container avoids it as it did before. The floor is inherent, the product hits exactly it, and §12.1's thresholds now have a number measured through the code that will be under test.

### §16 #13's decode half is done

Fedora ships libheif without HEVC, so §1's native subject does not open on a stock install and the symptom is three layers from the cause. The decoder consults libheif's codec list **before** it reads, against the container's own `ftyp` brand — so an AVIF in a `.heic` is checked for AV1 rather than HEVC — and a missing one is `MissingCodec` naming `libheif-freeworld`. A truncated file is still `Broken`, and telling those two apart is the whole of the item. The packaging clause itself is still open.

### Two things the tests found

**A grey fixture cannot tell sRGB from Display P3.** The first version of the "an untagged file is sRGB and a tagged one is read" test used a flat mid-grey JPEG and asserted the two interpretations differ. They do not, and cannot: **a neutral is neutral in every RGB space sharing a white point**, so (128, 128, 128) read either way lands on the same working-space value to the last bit. The test now uses a saturated red, and the grey fixture proves the complementary thing — that a neutral stays neutral.

**Hand-rolling a fixture for a format with tables in it is a bad trade.** The first minimal JPEG was written by hand and rejected with "invalid length in DHT". It is now 629 bytes generated once by Pillow and inlined, which is what a decoder test needs: a file a real decoder accepts.

### Opened

**§16 #17 — PNG, and therefore screenshots.** §1 names a screenshot as a native subject and §4 gives it a colour rule; v0.1 decodes HEIF and JPEG, so that rule is currently exercised by an untagged JPEG rather than by the file it was written for. One decoder against a colour path that already exists.

---

## 2026-09-06 — v0.1's render path, and §12.2 measured; spec v0.16 → v0.17

A compiled graph now renders. `engine/render.rs`, `shaders/photodesk/{adjust,encode}.wgsl`, 8 tests. The workspace is at 110.

### The product's shaders exist, and the spike's stay a spike

§13 names `shaders/photodesk/` as "the only shader source". Until now it was empty and the two WGSL files lived in `tests/renderer/shaders/` — but `encode.wgsl` was never a spike artefact, it was §16 #11's stage 13 written in the wrong place. It has moved, and the spike's lowering and agreement tests now check the **shipped** shader rather than a copy that happens to look like it.

`adjust.wgsl` is new, and is not Spike C's. That one was built to be *hostile* to naga — a large uniform block, dynamic indexing, a data-dependent loop, a switch — because §2.3's question was whether a realistic pass could lower at all. The product's implements what `adjust` at `op_version` 1 actually carries: the nine scalars of §5 stages 2–5 and 9. **Stages 6, 7 and 8 are absent and keep their numbers**, because §5's ordering is the frozen contract and a version that does not carry a stage runs identity there — the same thing §6.1 already says about an omitted key.

### There is no CPU renderer, and there will not be one

The obvious way to test a GPU renderer is to write the same maths in Rust and compare. That is exactly what §0 freezes against: two implementations of one pipeline drift, undiscoverably, because both look right alone. So the shaders are the only description of what a pixel goes through, and correctness is established the way the specification says — by rendering the same document two ways.

Two constants in `adjust.wgsl` are the exception that proves it. Linear Display P3's luminance weights and its XYZ matrix are hardcoded there rather than passed in a uniform, because a uniform would have to be filled by both the exporter and the preview — two places to write one number. A test reads them out of the shader text and compares against the values `colour.rs` derives from chromaticities, so the copy cannot drift.

### §12.2, and two wrong diagnoses before the right one

> Render the same document at proxy and at full-res-downsampled-to-proxy. Assert they match within threshold. **This is the test that enforces the one-shader-source invariant. Without it the invariant is a comment.**

`adjust` v1 has none of the spatial stages §12.2's carve-outs are written for, so the expectation was near-exact agreement. The first run said **57 codes with no adjustment at all**.

- *First diagnosis: the adjustment chain does not commute with the average.* Wrong — the control with an identity document was just as bad.
- *Second: it is clipping.* That predicted the disagreement would sit at the rails. Splitting the measurement showed 56 codes **away** from them and 8.6 at them, so also wrong.
- *Third, and it holds:* **the gamut map.** §16 #11's constant-luminance chroma clip is not linear, and a quick two-pixel check off to the side put a number on it — one neighbour outside sRGB and one inside is worth ~18 codes by itself.

So the threshold cannot be a number; it has to be stated against a **precondition**. There are exactly two ways the paths can differ, and a fixture that violates either stops being a regression test and becomes a measurement of an inherent property:

1. **Detail finer than the proxy**, which makes `mean` lossy, so `f∘mean ≠ mean∘f` for any non-linear `f`.
2. **Content outside the destination gamut**, where the map has a derivative discontinuity at the boundary — averaging across that kink diverges even on perfectly smooth content.

| content | worst | mean |
|---|---|---|
| band-limited **and** in-gamut | **0.1486** | 0.0355 |
| detail finer than the proxy, full chain | 152.53 | 11.31 |
| the same detail, stage 13 alone | 107.86 | 3.27 |

*8-bit codes, 256² reduced to 64².* With both preconditions held the two paths have no way to disagree, and they do not — 0.15 of a code is the RGBA16F attachment's own truncation, which `encode_stage.rs` already characterised. That is where the bar sits, and a spatial stage that broke it would show up there first.

The preconditions are **asserted in the fixture**, not assumed: if the band-limited image ever drifts near a rail the test says so, because otherwise the bar would quietly stop being about the renderer.

The rest is inherent to proxy editing, every editor since 2007 has it, and it is not a lie the user can see — §7.1 sizes the proxy at twice the viewport, so detail the proxy cannot hold is detail the screen cannot show.

### The sliders, and what is a choice

Six of the nine parameters are v0.1's controls. Each has a stated formulation and several are choices rather than facts, so §16 #4 now names them: contrast is a gain about 18% grey **in linear light**; stage 4's three controls are weighted on *perceptual* rather than linear luminance, because 0.5 linear is 73% encoded and thresholds in linear would put "midtone" up among the highlights; vibrance measures existing chroma relative to brightness so a saturated shadow counts as saturated.

**White balance is the one that took work.** Temperature is a Kelvin delta (forced by §5 — the stack is the loop, so two layers each declaring an absolute 5200 K would describe nothing), applied as a von Kries scaling between two points on the CIE daylight locus. Two properties are tested rather than hoped for:

- **Its zero is exactly the identity.** The gain is the ratio of the target white to *the locus evaluated at the reference*, not to D65's defined chromaticity — so a temperature of 0 gives 1.0 per channel rather than something within a thousandth of it. A stack of nine untouched sliders moves the picture by less than 10⁻⁵.
- **It does not change brightness.** The gain is normalised against the working space's luminance weights, measured at 0.17984 against 0.18 across ±2000 K. §11 gives each control one job and brightness is exposure's.

The formulations are where §12.1's golden images will earn their keep, which is why they are named in §16 #4 rather than left as shader comments.

### What is refused

`geometry` (v0.2), `composite` (layer opacity and masks, v0.4) and the four mask node kinds are **named errors**, not skips. Rendering a masked document without its mask produces a picture that looks plausible and is wrong, which is the same reason §6.3 rejects an unknown `op` rather than ignoring it.

The renderer asks for `downlevel_webgl2_defaults` rather than what the adapter offers, so a graph that exports is a graph the preview can run. Asking for more would let the export path succeed on something the webview refuses — the WYSIWYG drift §0 freezes against, arriving through the back door.

### Not yet

Full-resolution export wants **tiling** (§7.1) and this renders whole images. At proxy — where §12.2's comparison and the entire preview path live — whole-image is what is wanted anyway. Node textures are released as soon as their last consumer has run, which is what keeps §7.3's 512 MB plausible: holding every intermediate of a six-layer masked graph at 12 MP would be 1.4 GB.

---

## 2026-09-06 — v0.1's export, and §12.3 runs end to end; spec v0.17 → v0.18

A rendered frame becomes a file. `engine/{exif,export}.rs`, an ICC writer in `icc.rs`, 12 new tests. The workspace is at 122.

### §12.3 has all five of its steps for the first time

> `hash_before = blake3(file)`; open → apply document → render preview → export → close; `assert blake3(file) == hash_before`. **This is invariant #1 and it's the cheapest possible test for the most expensive possible bug.**

`sidecar.rs` has asserted the writing half since the document model landed. The whole chain now exists to run, and it does: open a JPEG with EXIF, save a sidecar, render a proxy preview, render at full resolution, encode a file, re-hash. Identical, and the mtime with it — a rewrite with identical bytes is still a rewrite and it is the kind that survives a hash comparison. The test carries its own counterexample so a hash that cannot see a change is not mistaken for one that passed.

### The tag we write is the one Apple writes

§4's chain ends "→ ICC-tagged file", and the profile is built in-tree for the same reason the parser reads in-tree: the harness has to be able to *disagree*. lcms2, which has never seen the writer, reads our Display P3 profile's red colorant as **(0.5151, 0.2412, −0.0011)** — the same D50 triple §4 recorded off a real iPhone. A transform built from our profile agrees with lcms2's own idea of the space to max ΔE 0.0030 over the 24 patches, and the profiles are 444 and 456 bytes against Apple's 536.

Two header fields are written as zeros on purpose: the creation date and the profile ID. **A timestamp would make every export of the same document a different file**, which breaks "export again, same result" and would make §12.1's golden images unblessable — a reference that differs from the render by the second it was made in fails every time. Now a register item.

### Export surfaced an orientation bug in the decoder

`metadata: strip` is §6.1's most destructive policy and the one whose correctness is least obvious. Writing it turned up something the decode unit had missed entirely: **the decode path did not read EXIF orientation at all.**

That is fine while the tag travels with the file and only becomes wrong when it does not. A photograph whose pixels are sideways and whose tag says "rotate me" reads correctly to anything honouring the tag — and `strip` removes the tag, so the export comes out rotated. Orientation is *structure*, not description, and the policy is about description.

So the decoder turns the pixels and the exporter writes orientation 1. All eight EXIF cases, including the four mirrors — rare from a camera, common from a scanner or a front-facing lens, and silently wrong if only the rotations are handled. libheif already applies the container's `irot`/`imir` during decode, so only the JPEG path needed it; that asymmetry is recorded rather than assumed.

Now a register item, because it is the reason a whole policy is honest.

### What `keep-minus-gps` actually has to do

Two things, and neither is the obvious implementation.

**Remove, do not unreference.** Deleting IFD0's pointer to the GPS block makes the coordinates unreachable through the tag tree and leaves them in the file. Anything that walks the segment rather than the structure still finds them, which is not what a privacy default means. The TIFF block is rebuilt from the entries that survive: 262 bytes becomes 136, and what is gone is gone.

**Drop the thumbnail, under every policy.** IFD1 carries a preview of the *source*. Carried into an export it shows a file browser the unedited photograph — and after a crop it hands back exactly what the crop removed. A stale preview is confusing; a crop that does not crop is a leak.

Byte order is preserved rather than normalised, deliberately: a TIFF block declares its own endianness and every multi-byte value follows it, so re-emitting in a fixed order would mean byte-swapping each value by type — correctly, for every type, including ones this code has no other reason to understand. Keeping the source's order means the value bytes are copied verbatim and cannot be corrupted by a tag nobody anticipated.

### Refused rather than substituted

TIFF is in `OutputFormat` and is not written: §1's non-goals put print workflows out of scope and no release has claimed it. Silently writing a PNG where a document asked for a TIFF is the kind of helpfulness that becomes a support question.

### Two fixtures, one lesson repeated

The EXIF fixture is generated and inlined, like the JPEG before it — and the generator was wrong on the first attempt in exactly the way hand-built binary formats are: `ifd()` never wrote the four-byte next-IFD pointer, so every offset after IFD0 was short by four and Pillow read the camera make as `e iPho`. The lesson from the decode unit stands: for a format with an offset table in it, generate the fixture and check it with something that did not build it.

### Opened

**§16 #18 — tiled full-res export.** §7.1 describes export as tiled and this writes whole images. The renderer releases a node's texture as soon as its last consumer has run, so a 12 MP export is a few hundred megabytes against §7.3's 512 MB cap — comfortable, and not the streaming path §7.1 describes. Due before a 60 MP source, or before v0.7's batch export makes the peak matter.

**§16 #19 — HEIF output, and EXIF for HEIF sources.** §6.1's formats are JPEG, PNG and TIFF, so a HEIC-in-HEIC-out round trip is not among them, and the metadata policy currently reads EXIF from JPEG only. §4's default output is sRGB JPEG for a reason — "that's what survives contact with the internet" — so this waits until a HEIF export is actually wanted.
