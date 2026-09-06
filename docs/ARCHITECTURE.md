# PhotoDesk — Architecture Specification

**Version:** 0.20
**Author:** Luis Howin
**Platform:** Fedora Workstation / GNOME
**Status:** Master spec for the coding agent. **Phase 0 complete; v0.1 runs.**

---

## 0. Governing principle

> **Freeze interfaces and invariants early. Freeze implementation choices only after a spike proves them.**

Two operational rules follow, and the agent is held to both:

- **Every frozen item carries a written reason.** If it can't be justified in a sentence, it isn't frozen — it's a habit.
- **Every provisional item names the spike that resolves it.** "Provisional" without a named exit condition is just indecision with better PR.

### The register

This table is the contract. Anything not listed is undecided and needs a decision recorded here before code depends on it.

| Item | State | Reason / exit condition |
|---|---|---|
| Source file is never modified | **FROZEN** | Non-destructive is the product. Enforced by test (§12.3). |
| One shader source, preview and export | **FROZEN** | Separate paths guarantee undiscoverable WYSIWYG drift. Enforced by test (§12.2). |
| Document is declarative, versioned, human-readable | **FROZEN** | Presets, history, batch and agent editing all fall out of it for free. |
| Pipeline order is explicit and versioned | **FROZEN** | The *contract*. Implicit order is unmigrateable. |
| AI is removable without touching the render path | **FROZEN** | Protects the editor from the volatile part. |
| Every cache is regenerable | **FROZEN** | No cache is ever load-bearing state. |
| Display-referred philosophy (§1) | **FROZEN** | It's the product thesis, not an implementation. |
| Provider/Segmenter interfaces exist | **FROZEN** | Cheap now, structural later. Implementations are not frozen. |
| Masks read the layer's input (§8) | **FROZEN** | Unnamed, it gets chosen accidentally and differently in preview and export. |
| Spatial radii source-relative; grain seeded, and carved out of §12.2 | **FROZEN** | §12.2 fails by construction otherwise, then gets muted. |
| History is session-only, never serialised | **FROZEN** | A sidecar accumulating every gesture grows without bound (§6.1). |
| Cache paths are never written into the document | **FROZEN** | Corollary of "every cache is regenerable". A document naming a disposable file has a dangling reference. |
| Providers run on a worker pool; `health()` is cached | **FROZEN** | §11 forbids blocking the canvas, and the trait signatures are synchronous. |
| **Do not fork RapidRAW** | **FROZEN** | Spike A's gate failed 2 of 4 criteria (§2.1, `FORK-AUDIT.md`). Stage order is the statement order inside one vendored compute kernel, so "pipeline order is explicit and versioned" is unimplementable in a fork. |
| **Build against `rawler` + `libheif` + our own shaders** | **FROZEN** | The §2.1 fallback, now the path. RapidRAW has no colour management and cannot open HEIF — §4 and §1's native subject were both greenfield inside the fork too. |
| **Working space = linear Display P3, f16** | **FROZEN** | Spike B measured it (§2.2, `SPIKE-B.md`): ΔE 0.0956 max at thirty passes, under a tenth of the ΔE 1.0 budget. §2.2's f32 fallback is not needed and should not be built. |
| **Preview renderer path: WebGL2 + WGSL→GLSL via naga** | **FROZEN** | Spike C (§2.3, `SPIKE-C.md`). Lowers, compiles in WebKitGTK, 5.92 ms for six layers at 2 MP against a 16 ms budget, and agrees with the wgpu path to max 0.0088 across 196,608 samples. §7.2's candidate, not its fallback. **Re-run 2026-09-06 in webkit2gtk-4.1**, the binding Tauri v2 embeds: identical capabilities and identical agreement to every digit. |
| **Shaders are authored fragment-first** | **FROZEN** | `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns. Compute, storage buffers and storage textures have no GLSL ES 3.0 target — naga refuses them by name — so using one anywhere breaks the preview path for every shader. |
| **v1 pipeline stage ordering** | PROVISIONAL | → golden-image validation (§12.1) |
| Document is stack-shaped on disk | PROVISIONAL | Until the stack becomes lossy for the graph (§6.2) |
| Which AI providers ship | PROVISIONAL | Interface frozen, implementations swap freely |
| **v1 discards iPhone HDR gain maps** | PROVISIONAL, **now exercised** | → an HDR display, or the first wanted gain-mapped export (§4). Confirmed against a real iPhone HEIC: the gain map is an auxiliary image under `urn:com:apple:photo:2020:aux:hdrgainmap` at half resolution, libheif's default decode returns the SDR base without applying it, and the discard is now a skip we can see rather than one we assume. |
| **ICC extraction from real containers** | **FROZEN** | Measured (§2.2, `tests/color/tests/heif_icc.rs`): a Display P3 ICC survives a real HEIF container byte-identical, parses back to P3's red primary at X 0.5151 rather than sRGB's 0.4361, and drives the transform to max ΔE 0.41 — where ignoring it costs 3.43, so the test can tell the two apart. |
| **HEVC decode needs `libheif-freeworld`** | **FROZEN as a platform fact** | Fedora's stock libheif ships no HEVC codec at all (patent policy) — measured, not assumed. Installed here, and asserted by `tests/color/tests/heif_codecs.rs` so a mis-provisioned machine says so rather than failing to open a photograph. Consequences for §13 packaging below. |
| **HEIC sources carry a ~0.9 ΔE conversion floor** | **FROZEN as a format fact** | libheif converts RGB↔YCbCr around every YCbCr codec. Measured identical to four decimals across libaom and x265, both asked for lossless — so it is the conversion, not compression. Apple ships YCbCr, so it is unavoidable on read. §12.1's HEIC thresholds must sit above it. |
| **Export gamut mapping = clip chroma at constant luminance** | **FROZEN** | Measured (§4, `tests/color/tests/gamut_policy.rs`). The clip's error has no policy — how much lightness a colour loses depends on which channel ran out first, up to 2.8 L\*. This one's error is *stated*: L\* is exact by construction, chroma is what gets spent. On a real photograph it halves the adjacent pixel pairs that merge into one colour, for zero cost inside the gamut and the same handful of ALU ops. |
| **Stage 13 runs the gamut map in the fragment shader** | **FROZEN** | Corollary of "one shader source, preview and export": the preview gamut-maps every frame to the display, so a policy that needs a per-pixel search is not adoptable whatever its colorimetry. Checked, not assumed — `shaders/encode.wgsl` lowers to GLSL ES 3.00 and agrees with the Rust reference to one colour-attachment step (`tests/renderer/tests/encode_stage.rs`). |
| **The document schema's home is Rust; the TypeScript types are generated from it** | **FROZEN** | §12.3's source preservation and §12.1's golden images have to run headless on every commit — which is criterion 1 of the gate Spike A failed RapidRAW on, and a schema reachable only through the webview fails it the same way. A hand-written TypeScript twin would make "one schema" untestable exactly as a hand-written GLSL twin would have made "one shader source" untestable, so the front end reads generated declarations (`src/document/generated/`, committed, staleness caught by a test). |
| **Exports are deterministic** | **FROZEN** | Same document, same bytes. No timestamp in the ICC profile, no encoder state that depends on when it ran. §12.1's golden images cannot be blessed otherwise — a reference that differs from the render by the second it was made in fails every time it runs. |
| **EXIF orientation is applied to the pixels, never carried forward** | **FROZEN** | Orientation is structure, not description. A file whose pixels are sideways and whose tag says "rotate me" reads correctly only to software honouring the tag, so §6.1's `metadata: strip` would rotate the photograph. The decoder turns the pixels; the export writes orientation 1. |
| **Proxy and export agree exactly only on band-limited, in-gamut content** | **FROZEN as a property** | Measured (§12.2, `src-tauri/tests/render.rs`). Under both conditions the two paths differ by **0.15 of an 8-bit code**; with detail finer than the proxy, by **152**, of which 108 is the gamut map alone. Neither is a defect — a non-linear chain does not commute with an average, and §16 #11's map has a kink at the gamut boundary. §12.2's thresholds have to be stated against the preconditions rather than against a number. |
| **ICC profiles are parsed in-tree, not by lcms2** | **FROZEN** | The code deciding how a photograph is interpreted should be code this project can read, and the harness should be able to disagree with it — `tests/color/` checks this parser against lcms2 over the same bytes and they agree to **7.4 × 10⁻⁹**. Same "two links, both tested" arrangement §2.2 uses for ΔE2000 and the matrices, and it keeps a C library off the shipping path for eighty lines of byte reading. |
| **A profile that is neither sRGB nor Display P3 is refused, not rounded** | **FROZEN** | §4 handles two spaces and the register says a third is a decision, not a value. Classified on the whole 3×3 colorant matrix after Bradford D50→D65, tolerance 0.02 — derived, not chosen: Adobe RGB sits 0.0901 from Display P3 and *nearer to it than to sRGB*, so a threshold on the red colorant alone reads Adobe RGB as Display P3, which is a silent wrong colour on a profile people have. |
| **The graph is compiled once, in Rust; both renderers execute the same plan** | **FROZEN** | Corrects §13, which puts "DAG compile" in the front end. The preview runs in the webview and the export runs through wgpu (§7.2) — two compilers would render two topologies and drift exactly as two shader sources would, with §12.2 then comparing two *compilations* rather than two executions of one plan. The one-shader-source invariant would be enforced over the shader while the graph above it went unchecked. `src/graph/` is the plan's executor. |
| **Sidecar keeps the whole filename: `IMG_4821.HEIC.photodesk.json`** | **FROZEN** | Corrects §6.1's example, which drops the extension. §14's v0.1 is "open a HEIF → … → export", so `IMG_4821.jpg` beside `IMG_4821.HEIC` is the workflow rather than a corner case — and under §6.1's naming those two share one sidecar, so editing the export silently overwrites the original's edits. |
| **RGBA16F colour attachments may truncate rather than round** | **FROZEN as a platform fact** | Measured on RADV/RENOIR: 48,020 of 49,152 stored channels are bit-exactly the reference *truncated toward zero*, not rounded to nearest. It is the driver's rounding mode, not the shader's arithmetic, so §12.1's thresholds have to allow one attachment step or a correct render fails the suite (§16 #15). |
| **An alpha channel is composited onto white, in linear light, at decode** | **FROZEN** | The pipeline has no alpha channel and v1 will not grow one, so a file that has one is resolved at the door. Onto white rather than dropped, because dropping is not neutral — the RGB under a transparent pixel is whatever last wrote there, so a window screenshot's rounded corners would arrive carrying arbitrary colour. In linear light because that is the only place the arithmetic is right: half-transparent black over white is 0.502 linear, and the same composite on encoded values is 0.216. Reported on `Decoded` for the reason the gain-map skip is (§4). |
| **One conversion path from a decoded buffer to the working space** | **FROZEN** | Three decoders hand back three shapes — libheif pads its rows, PNG can be grey, sixteen-bit, paletted or transparent — and the alternative to describing them is a conversion per format. Same argument as one shader source, at a smaller scale and with the same failure mode: two paths agree until they do not, and nothing is checking. Container features are flattened *before* the chain (§4), which is what keeps it one path rather than one path with branches. |
| **PNG's `cHRM`/`gAMA` are not read as a colour tag** | PROVISIONAL | → a file that carries them without `iCCP` or `sRGB`, in a corpus rather than in the abstract. The two chunks that *name* a space are read; these two *describe* one numerically, and reading them means a nearest-space classifier — which is the shape §4 has already been bitten by once, Adobe RGB sitting nearer to Display P3 than to sRGB. Until such a file exists the cost of not reading it is that it is treated as untagged, which is §4's stated guess anyway. |
| **The window is a separate crate from the library** | **FROZEN** | `photodesk` has no `tauri` dependency and never will; `photodesk-app` has both. §12.3's source preservation and §12.1's golden images run headless on every commit — criterion 1 of the gate Spike A failed RapidRAW on — and a crate that links webkit cannot run them. An optional `tauri` feature would have kept the layout tidier and left `cargo test --all-features` able to break the invariant quietly. A crate cannot be linked into the tests by accident. |
| **The preview lowers WGSL at startup, not at build time** | **FROZEN** | A generated `.frag` on disk is a second artefact that can be stale, and a stale one is the preview rendering last week's shader while the export renders this week's — the WYSIWYG drift §0 exists to prevent, arriving through a forgotten build step rather than through a second source file. `engine::glsl` runs in a few milliseconds, once, and there is nothing to keep in sync. It is product code, so `tests/renderer/` calls it rather than keeping a copy. |
| **Uniform offsets are read back from the linked program, never tabulated** | **FROZEN** | naga renames `@group`/`@binding` into whatever GLSL ES 3.00 can express — `_group_0_binding_0_fs.exposure` today — so a table of offsets in TypeScript would be a second description of a layout naga owns, going stale silently as one slider that does nothing. The front end asks the driver's own reflection of the shader that is about to run. Checked by `tests/renderer/`, which asserts the lowered source still names all nine of the document's parameters. |
| **No front-end framework: TypeScript + Vite, zero runtime dependencies** | **FROZEN** | The canvas needs none (Spike C), and §10/§11 specify the interaction surface closely enough that a component library would be overridden rather than used. Cost accepted knowingly: panels, undo and the keymap are hand-written, and the bill arrives at v0.2–v0.7, not v0.1. |

---

## 1. Philosophy

PhotoDesk is a **display-referred photo editor**. Its native subject is a finished image — an iPhone HEIF, a JPEG, a screenshot — that already has a rendering intent baked in by the device that made it.

It is **not** a RAW laboratory. It doesn't reconstruct scene radiance, doesn't ask you to pick a demosaic algorithm or a view transform. RAW is an *input*, not a worldview.

**The test for any feature:** does it shorten the path between opening a photo and being happy with it?

**Non-goals:** cataloguing, cloud sync, face recognition, tethering, print workflows, compositing, scene-referred view transforms, camera profile management, demosaic pickers, soft-proofing UI, a node editor UI, plugins for other people.

---

## 2. Phase 0 — three spikes before any product code

Nothing in §4 onward is safe to build until these three questions are answered. Budget roughly **three weeks with no visible product**. That is not wasted time; it's the price of not discovering any of this in month four.

**Phase 0 is complete. All three spikes have run.** A failed its gate, which was worth having in week one (`FORK-AUDIT.md`). B is green and the working space is frozen (`SPIKE-B.md`). C is green and the preview path is frozen (`SPIKE-C.md`). §2.1–§2.3 below are kept as written — the questions they asked were the right ones, and two of the three answers were not the expected ones.

### 2.1 Spike A — RapidRAW fork audit — **COMPLETE, GATE FAILED**

> **Result (2026-09-05):** criterion 1 passes, criterion 2 passes on its wording while the invariant behind it fails, criteria 3 and 4 fail. **No fork.** Three further findings — no colour management anywhere, no HEIF support, and a Linux preview that is a per-frame JPEG round-trip — say the fork would not have supplied §4, §1's native input format, or a preview path either. Full evidence, subsystem table and register consequences in [`FORK-AUDIT.md`](FORK-AUDIT.md). The section below is preserved as the question that was asked.

Do not assume the Rust core separates cleanly from the front end. In a solo-built Tauri app the "backend" is frequently organised around front-end-shaped IPC commands, in which case there is no engine to fork — only an application to read.

**Classify every subsystem before committing:**

| Class | Meaning |
|---|---|
| `KEEP` | Vendored unmodified. Tracked upstream. We never edit it. |
| `ADAPT` | Kept, wrapped behind a PhotoDesk interface. Upstream changes merge with effort. |
| `REPLACE` | Rewritten. Upstream version is reference only. |
| `AVOID` | Not used. Note *why*, so it isn't reintroduced later. |

**Subsystems to classify, minimum:** RAW decode (`rawler` integration), demosaic, colour management and ICC handling, the WGSL shader chain, GPU resource/texture management, the mask rasteriser and compositor, edit-state representation, sidecar/persistence, preset system, history/undo, thumbnail and proxy generation, export and encoding, AI/ComfyUI bridge, IPC command surface, file IO and watching.

For each, record: lines of code, dependency fan-out, whether it can be called without the front end present, and how tightly it's coupled to their edit-state struct.

**Already measured** (RapidRAW 1.6.3, AGPL-3.0), so the audit starts here rather than from zero:

| Observation | Consequence for the audit |
|---|---|
| 35,601 lines of Rust in a flat `src-tauri/src/*.rs` layout | Module boundaries are file boundaries. There is no crate seam to fork along |
| `file_management.rs` carries 43 of the `#[tauri::command]`s | The IPC surface is concentrated — a rind, not a marbling. Good news for criterion 1 |
| `image_processing.rs` 3 Tauri references, `gpu_processing.rs` 7, `mask_generation.rs` 4 | The processing core is only lightly coupled to the front end |
| `export_processing.rs` 24 Tauri references, 11 `AppHandle` | Reads `REPLACE` on sight |
| Edit state is `GlobalAdjustments` / `MaskAdjustments` / `AllAdjustments`, `image_processing.rs:1472–1617` | The concrete target for the criterion-2 adapter |
| **The shader chain is compute, not fragment.** `shaders/shader.wgsl:1616` is `@compute @workgroup_size(8,8,1)`, writing `texture_storage_2d<rgba8unorm, write>` and reading `var<storage, read> adjustments`. `blur.wgsl` and `flare.wgsl` likewise; only `display.wgsl` has a `@fragment` entry | **The finding that matters.** WebGL2 has no compute shaders, no storage textures and no storage buffers. See §2.3 |
| The output storage texture is `rgba8unorm` | 8-bit, against the f16 working space of §4. A second `REPLACE` signal for the chain |
| `rawler` is itself a fork (`CyberTimon/RapidRAW-DngLab`), not upstream | `KEEP` here means tracking somebody else's fork, not an upstream. Say so in the table rather than discovering it at the first rebase |
| `wgpu` pinned to 29.0, commented "downgraded to prevent P3 color shifts on Apple devices" | Their colour path has known version sensitivity. Feeds Spike B |

**Classify the shader chain explicitly, and return exactly one answer.** §13 lists both `src-tauri/src/engine/` (vendored, read-only) and `shaders/photodesk/` ("WGSL source of truth"). Both cannot render. Spike A decides which, and the losing directory does not exist in the tree.

**Gate criteria — the fork proceeds only if all four hold:**

1. The processing chain can be invoked headlessly from a test binary, with no webview and no front-end state.
2. Their edit-state struct can be replaced by ours behind an adapter without editing shader code.
3. At least RAW decode, demosaic and the core shader chain classify as `KEEP` or `ADAPT`.
4. **PhotoDesk's stage order (§5) is expressible without editing vendored shaders.** Criteria 1–3 can all pass while this one fails, because stage order is encoded *inside* the shader chain and `engine/` is declared read-only (§13). If the order we want requires editing a file we have promised never to edit, then "pipeline order is explicit and versioned" — a FROZEN item — is unimplementable, and the fork has quietly taken the contract away.

**If the gate fails:** don't fork. Use it as an architectural reference and build against `rawler` + `libheif` + your own WGSL directly. That's slower — but a fork you have to rewrite is slower still, and you'd have discovered it in month four instead of week one.

**Deliverable:** `docs/FORK-AUDIT.md` with the table filled in and the gate explicitly passed or failed.

### 2.2 Spike B — colour validation harness — **COMPLETE, GREEN**

> **Result (2026-09-05):** all four tests pass, plus two cross-validations against lcms2. f16 storage costs **ΔE 0.0956 max at thirty passes** — under a tenth of the ΔE 1.0 budget — so the working space is frozen and the f32 fallback is not needed. The deep-shadow ramp is bit-exact. **Two of six corpus items are blocked on `libheif-devel`**; both test ICC extraction from a container rather than the working space, and now have their own register entry. Full numbers in [`SPIKE-B.md`](SPIKE-B.md); harness in `tests/color/`.

Linear Display P3 f16 is the leading candidate (§4), not a decision. Prove it.

**Corpus:** a synthetic 24-patch chart with exact known values in sRGB and in Display P3; a real iPhone HEIF with an embedded P3 profile; **an iPhone HEIC carrying an ISO HDR gain map** (§4); an untagged screenshot; a wide-gamut synthetic gradient; a deep-shadow ramp (values 0–16/255) for precision testing.

**Tests, all automated, ΔE2000:**

| Test | Threshold | What it catches |
|---|---|---|
| Round-trip identity: decode → working space → encode, zero adjustments | max ΔE < 1.0 | Broken transforms, wrong EOTF assumptions |
| P3 source → sRGB export, compared to a reference converter | mean ΔE < 1.5 | Gamut mapping errors |
| Untagged input assumed sRGB | max ΔE < 1.0 | Silent misinterpretation |
| **Deep-shadow ramp through the full chain** | no banding (defined below), max ΔE < 2.0 | **f16 precision in linear light** |

**"No banding" needs a number, or it is not an automated test.** Define it on the 0–16/255 output ramp as: monotonic non-decreasing; at least 14 distinct output codes across the 17 input steps; and no second difference exceeding one code. Those three catch flattening, quantisation collapse and stair-stepping respectively, and all three are assertions rather than opinions.

**The one that matters most is the last.** Half float has ~11 bits of significand, but *linear* encoding distributes those codes badly in the low end — most of the precision sits in the highlights where the eye cares least. For 8-bit-sourced material it is very probably fine, and "very probably fine" is exactly what a harness exists to convert into a fact. If it bands, the fallback is f32 working buffers at proxy resolution (still cheap at 2 MP) or a non-linear working encoding.

**Exit:** harness green → freeze the working space. Harness red → the register entry changes and §4 is rewritten before anything depends on it.

### 2.3 Spike C — preview renderer — **COMPLETE, GREEN**

> **Result (2026-09-05):** the WGSL lowers to GLSL ES 3.00, WebKitGTK compiles it, `EXT_color_buffer_float` is present with RGBA16F colour-renderable and linear-filterable (measured, not inferred), six layers cost **5.92 ms at 2 MP** against a 16 ms budget, and the WebGL2 and wgpu paths agree to **max 0.0088** across 196,608 channel samples. **§7.2's candidate is frozen, not its fallback.** A compute shader of RapidRAW's shape is refused by naga naming `BUFFER_STORAGE | COMPUTE_SHADER | IMAGE_LOAD_STORE` — so the fragment result is a consequence of authoring fragment-first, not a coincidence. Full numbers in [`SPIKE-C.md`](SPIKE-C.md); harness in `tests/renderer/`.
>
> **Confirmed in Tauri's own binding (2026-09-06).** Spike C measured in Epiphany, which is webkitgtk-6.0 on GTK 4; Tauri v2 embeds **webkit2gtk-4.1** on GTK 3, and `SPIKE-C.md` left that gap open to the first v0.1 build. It did not have to wait: `run-probe.py --engine webkit2gtk-4.1` drives a WebView directly, and the two bindings return **identical capabilities, an identically compiled shader, and pixel agreement identical to every digit** — max 0.008789, mean 0.0001944, the same three channels over 1/255. Six layers cost 6.17 ms against 5.92, a 4% difference inside a 16 ms budget. The residual risk on the frozen preview path is retired.

Establish whether the same WGSL can drive both paths on Fedora (§7.2). Success is a slider moving an image at proxy resolution inside the webview, at budget (§7.3), **and the working space actually representable** — RGBA16F as a colour-renderable target with linear filtering, which on WebGL2 means `EXT_color_buffer_float` on this machine's WebKitGTK. That second clause is not padding: without it Spike B can freeze f16 against a preview path that turns out unable to render to it, and the two spikes would each be individually green and jointly wrong.

**Spike A reopened the first question.** The chain that was compute end to end — `@compute` entry points, a `texture_storage_2d` output, a storage buffer of adjustments — was *RapidRAW's*, and the fork is off (§2.1). None of that reasoning binds a chain we author ourselves.

So the branch that looked foreclosed is live again. **Author fragment-first**: `@fragment` entry points, uniform buffers instead of storage buffers, sampled textures instead of storage textures, rendering to a framebuffer attachment instead of `textureStore`. The constructs with no GLSL ES 3.0 target are then never used, and §7.2's "author once in WGSL, transpile with `naga`" is a candidate rather than a dead end.

Two things make that fit rather than merely allow it:

- **§5's per-layer loop is what makes a uniform buffer sufficient.** RapidRAW needed a storage buffer because it blends 32 mask parameter blocks in one pass. PhotoDesk runs the loop body once per layer, so a pass carries one layer's parameters — tone curves dominate at 4 × 16 points, and the block lands well under GLES3's guaranteed 16 KB uniform-buffer minimum.
- **§7.3 already requires stages 2–9 fused into one pass**, and a fused per-pixel pass *is* a fragment shader.

**This is a hypothesis and the spike has to test it, not assume it.** Run `naga`'s GLSL backend at `300 es` against a real fragment shader of ours before anything is claimed. The fallback is unchanged if it fails: hand-write the preview chain, roughly fifteen fragment shaders of well-understood per-pixel maths, and preserve one shader source by making the shared artefact the *shader body* — the maths in an included file, emitted for both targets — rather than the dispatch mechanism around it. If even that sharing proves unworkable, the thing under threat is §0's one-shader-source invariant itself, and it gets escalated to a register change rather than quietly patched.

Run this spike early. Its outcome decides how every shader in the project is written, and that is not a decision to discover twenty shaders in.

---

## 3. RAW is a prefix, not a second pipeline

RAW gets no mode, no panel set and no vocabulary of its own. Three stages in front of the normal pipeline, then it's just an image.

```
NEF/CR2/ARW/DNG ──► demosaic ──► camera matrix ──► working space ──┐
                                                                    ├──► PIPELINE
HEIF/JPEG/PNG   ──► inverse EOTF ──► matrix ──► working space ─────┘
```

- No "Develop" mode. No mode switch of any kind.
- No demosaic picker. The library default is correct until a golden-image test says otherwise.
- No camera profile browser. Auto-detect, apply, move on.
- Highlight *recovery* lives silently inside the prefix. It's a property of RAW data, not a slider. On JPEG, blown is blown, and the Highlights control compresses the upper range without pretending otherwise.
- Lens correction is prefix-only. The iPhone already did it; running it again on HEIF is wrong.

Opening a NEF gives the same tabs as opening a HEIF. That's the entire point.

**A platform fact that arrived with the first real container test, and is worth knowing before v0.1 rather than during it.** Fedora ships `libheif` with **no HEVC codec at all** — its encoder and decoder lists for HEVC are both empty, while AV1, AVC, JPEG and JPEG 2000 are all present. That is a licensing decision, not an oversight: HEVC is patent-encumbered and Fedora will not ship it. RPM Fusion's `libheif-freeworld` supplies it.

An iPhone HEIC is HEVC. So **the application's native subject does not open on a stock Fedora install**, and that is a dependency to declare rather than discover. See §13.

**A second fact came out of testing the container properly, and it sets a floor nothing downstream can go below.** libheif converts RGB↔YCbCr around every YCbCr codec, and that conversion costs about **ΔE 0.9 at worst, 0.29 mean** on a 24-patch chart. It is measurably *not* compression: libaom and x265 — different codecs, different implementations, both asked for lossless — return the same figure to four decimal places, and only the uncompressed container avoids it entirely. Apple does not ship uncompressed, so **every HEIC PhotoDesk opens carries this before the pipeline sees a pixel.** §12.1's golden-image thresholds for HEIC sources have to sit above it, and no amount of care in §4 or §5 buys it back.

---

## 4. Colour architecture

**Working space: linear Display P3, f16. FROZEN** by Spike B (§2.2, `SPIKE-B.md`) — measured at ΔE 0.0956 max through a thirty-pass chain, against a ΔE 1.0 budget.

**Export gamut mapping: clip chroma at constant luminance. FROZEN** 2026-09-06 (`tests/color/tests/gamut_policy.rs`, `tests/color/src/gamut.rs`).

A Display P3 photograph exported to sRGB produces channels outside [0,1] for everything the smaller gamut cannot hold, and something has to decide what happens to them. Until now that something was one `clamp` in a test harness.

**There is no off-the-shelf answer to defer to.** Perceptual rendering lives in a profile's B2A lookup tables, and neither sRGB nor Display P3 has any — they are matrix/TRC profiles. lcms2 returns the same transform to **ΔE 0.000000** whether asked for perceptual, saturation or relative colorimetric. Asking a colour engine for "perceptual" here returns the clip under a different name.

**And ΔE cannot choose between the candidates**, which is worth saying out loud because it is the measurement anyone reaches for first. Clamping each channel to [0,1] is exactly the Euclidean projection onto the gamut cube — so the clip *is* the nearest in-gamut colour, in linear RGB, a space nobody perceives in. The perceptually nearest colour, found by search, scores better still (mean ΔE 3.09 against the clip's 3.32) and is not shippable, because it is a per-pixel search. Both leave **81 of 81** out-of-gamut samples *on* the gamut surface, because minimising a distance is projecting to the surface whatever the distance is. The lowest ΔE and the flattest gradient are the same answer.

So the policy was chosen on what the error is made of rather than on how large it is:

| | clip each channel | **constant-luminance chroma clip** | the same, plus a soft knee at 0.95 |
|---|---|---|---|
| ΔL\* on out-of-gamut colour | up to **2.82** | **0.000** | **0.000** |
| ΔH, metric, same corpus | **10.73** | 25.36 | 25.57 |
| in-gamut colour | untouched | untouched | up to ΔE 1.01, over 3.2% of the frame |
| worst boundary ramp | **28 of 33 codes**, one step at ΔE 0.0000 | 33 of 33 | 33 of 33 |
| adjacent pairs merged, real photograph | **75** of 60,304 | **39** | 29 |

The chosen policy slides the colour along the ray from the achromatic point *of its own luminance* until it sits exactly on the gamut boundary. The vector it scales carries zero luminance by construction, so L\* comes through exact rather than nearly-exact and chroma is the only thing spent. That is the whole argument for it: **the clip's error has no policy.** How much lightness a colour loses depends on which channel happened to run out first, and a WYSIWYG editor cannot tell the user what became of their colour if the answer is "it depends".

**The cost, stated so it is not a surprise later.** On extreme saturation this policy moves hue and chroma *more* than the clip does — ΔH 25.4 against 10.7, on a corpus that contains the P3 primaries themselves. That corpus is hostile by design and no camera produces those colours; on a real photograph the gap is 0.13 ΔE of mean movement. It is nonetheless the direction this policy is weakest in, and a golden image showing a saturated red drift toward pink is the signal to reopen it.

**Why not the soft knee.** It is measurably the best at keeping gradients — 29 merged pairs against 39 — and it is rejected anyway. It buys those ten pairs by moving **3.23% of the frame that was already correct**, at up to ΔE 1.01, on the strength of a knee constant with no derivation behind it. §0's rule applies literally: a number that cannot be justified in a sentence is not frozen, it is a habit. The variant is implemented and measured (`GamutPolicy::CompressLuma`, with a knee sweep from 0.60 to 0.98) so reopening it is a one-line change rather than a fresh investigation.

**It runs where §0 requires it to run.** Stage 13 is the only stage every pixel of *both* paths goes through, so a policy that cannot be expressed in a fragment shader is not adoptable here whatever its colorimetry — which is the shape of the finding that killed the fork, asked in advance this time. `shaders/encode.wgsl` implements it, lowers to GLSL ES 3.00, and run through wgpu agrees with the Rust reference to **one colour-attachment step**: bit-exactly the reference truncated to f16 on 48,020 of 49,152 channels. A matrix multiply, a dot product, three divides and a min. No loop, no table, no search.

**Why not Rec.2020:** a container for a gamut this app will never receive on a display that can't show it. Sources are P3 and sRGB; outputs are P3 and sRGB. Rec.2020 spends precision on empty space and adds two matrix transforms per image for nothing.

**Why not scene-referred:** an iPhone HEIF has already been through Smart HDR and Apple's display rendering, and that tone mapping is not invertible. A scene-referred pipeline would try to un-bake it, fail, then bake a different one — which is the filmic/sigmoid rabbit hole that makes darktable feel like homework. Take the manufacturer's rendering as the starting point; edit *from* it.

**Why linear at all:** exposure, white balance and mask blending are only correct in linear light. Linear as a working buffer; display-referred as the mental model.

```
JPEG / HEIF   → embedded ICC → inverse EOTF → matrix → linear P3
Screenshot    → assume sRGB if untagged → inverse EOTF → linear P3
RAW           → demosaic → camera matrix → linear P3

linear P3 → tone encode → sRGB (default) or Display P3 → ICC-tagged file
```

Default export is sRGB, because that's what survives contact with the internet.

**The screenshot line is exercised by a screenshot now (§16 #17, closed 2026-09-06).** Until v0.1 the decoder read HEIF and JPEG, so "assume sRGB if untagged" was tested against an untagged *JPEG* — the right rule demonstrated on the wrong file. PNG closes that, and the colour work was the small half: the profile path is the one JPEG already walks, and the classifier, the untagged fallback and the working-space conversion needed nothing. What PNG brought was **container** features that carry no colour meaning — palette, sub-byte samples, greyscale, sixteen bits, Adam7 interlacing, and an alpha channel — and the question each one asks is where it gets resolved.

All of them resolve before §4's chain, and the chain itself did not grow a branch. `Transformations::EXPAND` flattens palette, sub-byte depth and `tRNS` inside the decoder; the one conversion function reads grey as three channels and sixteen bits through a 65,536-entry table instead of a 256-entry one. **Sixteen bits are kept**, because a PNG is the only input v0.1 reads that carries more than eight, and stripping it on the way into an f16 buffer chosen for headroom (§2.2) would be an odd thing to do.

**Alpha is the one that is a decision, and it is composited onto white.** The pipeline has no alpha channel and v1 will not grow one, so a file that has one must be resolved at the door — and dropping it is not the neutral option it looks like: the RGB under a fully transparent pixel is whatever the compositor last wrote there, so a GNOME window screenshot's rounded corners would come in carrying arbitrary colour rather than nothing. Compositing onto white gives them the page they will be looked at on. **In linear light**, which is the only place the arithmetic is right — half-transparent black over white is 0.502 of linear white, and the same composite done on encoded values lands at 0.216, less than half of it. That is a test rather than a remark.

**Two chunks name a colour space and PNG has four.** `iCCP` carries a profile and goes through the same classifier as everything else; `sRGB` names the space without carrying one, which is NCLX's arrangement in a different container, and it is reported as its own tag rather than folded into "untagged" — §4's guess is defensible for a file that says *nothing*, and a file that said sRGB has said something. `cHRM` and `gAMA` describe a space numerically instead of naming it, and v0.1 does not read them (register). One consequence worth stating: `png` drops an `iCCP` chunk it cannot inflate and then reports no profile at all, which arrives indistinguishable from an untagged file — so the chunk headers are walked to tell a damaged claim from no claim, and a damaged one is refused.

**Measured against real files (2026-09-06), and §4's assumptions all held.** An iPhone HEIC and an iPhone JPEG, straight off the device:

| | HEIC | JPEG |
|---|---|---|
| Colour tag | ICC, 536 bytes | ICC, 536 bytes in APP2 |
| Red colorant XYZ | (0.5151, 0.2412, −0.0011) | identical |
| Interpretation | **Display P3** | **Display P3** |
| Base image | 3024 × 4032, **8-bit** luma and chroma | — |
| Round trip through linear P3 f16 | **max ΔE 0.0000** over 3,072 real pixel samples | — |

Three things follow that were assumptions this morning. The manufacturer really does tag P3, in both containers, with the same profile — so §1's "take the manufacturer's rendering as the starting point" has something concrete to read. The base image is **8-bit**, which is what makes Spike B's f16 headroom argument apply to real material rather than only to synthetic ramps. And a real photograph survives the working space exactly: not ΔE 0.03, but 0.0000 across the sampled grid.

**The one number that moved is the export.** The same pixels taken to sRGB shift by **max ΔE 2.86, mean 0.29** — not an error, but real P3 content being gamut-mapped, and visible at the top end. That measurement is what turned §16 #11 from a tidiness item into a decision worth up to three ΔE on the user's own photographs, and it is the reason the policy above is now named here rather than in a test file. *Measured under clip-in-linear, which was the policy at the time; the figure moves when the corpus is next available.*

**HDR gain maps — v1 ignores them, and says so out loud.** A modern iPhone HEIC ships an SDR base image plus an ISO gain map that Photos.app applies on an HDR display. PhotoDesk v1 decodes the SDR base and discards the gain map. The reason it has to be *written down* rather than merely implemented: §1's thesis is to take the manufacturer's rendering as the starting point, and on an HDR display the manufacturer's rendering *is* the gain-mapped one — so an unstated drop means the app opens a photo looking flatter than the Photos.app the user just came from, and they conclude the colour pipeline is broken. It isn't; it's this decision. It is the right decision for v1 anyway, because the target display (§4) is a 60–70% sRGB laptop IPS that cannot show the difference. **Exit condition:** an HDR-capable display, or the first time a gain-mapped export is actually wanted. `image-hdr` is already in RapidRAW's dependency tree if that day comes.

**What the gain map actually looks like, now that one has been opened.** It is an auxiliary image under `urn:com:apple:photo:2020:aux:hdrgainmap`, carried inside the same container as the base image rather than as a second top-level image, at **half resolution** — 1512 × 2016 against the base's 3024 × 4032 — and 8-bit. Two consequences. libheif's **default decode returns the SDR base and does not apply it**, so v1's behaviour is what falls out of doing nothing, which is the safe direction; and if the exit condition is ever met, the map has to be **upsampled** to base resolution rather than sampled one-to-one. The discard is now a skip we can see rather than one we assume, which is what §4 asked for when it insisted this be written down rather than merely implemented.

**Reading the profile is ours to do, and the trap in it is the connection space.** ICC
colorants are stored in the profile connection space, which is **D50** — a Display P3
profile's `rXYZ` reads (0.5151, 0.2412, −0.0011) where Display P3's red primary at D65
is (0.4866, 0.2290, 0.0000). Comparing the tag against a D65-derived matrix finds
neither space and rejects every photograph this application exists to open, so the
colorants are Bradford-adapted before anything is compared.

Classification is on the **whole colorant matrix**, not on one number. The tolerance is
derived rather than picked: sRGB and Display P3 differ by 0.0934 at their largest
component, and Adobe RGB — the nearest common space PhotoDesk does *not* handle — sits
0.0901 from Display P3, which is **closer to P3 than sRGB is**. A threshold on the red
colorant alone therefore reads Adobe RGB as Display P3. At 0.02 a profile must be less
than a quarter of the way from P3 to Adobe RGB to be accepted as P3, while the tags'
own quantisation (s15Fixed16, ~1.5 × 10⁻⁵) is three orders of magnitude below that.

The tone curve is checked too. Primaries are half of a space: a profile with Display
P3's primaries and a 1.8 gamma is not Display P3, and both of §4's spaces use the sRGB
curve — Display P3 uses it rather than DCI's 2.6 gamma, and getting *that* wrong is the
~4 ΔE error that looks like a gamut problem and is not one.

**And the tag we write is the one Apple writes.** §4's chain ends "→ ICC-tagged file";
the profile is built in-tree for the same reason it is read in-tree, and lcms2 — which
has never seen the writer — reads our Display P3 profile's red colorant as (0.5151,
0.2412, −0.0011), the same D50 triple this section recorded off a real iPhone. A
transform built from our profile agrees with lcms2's own idea of the space to max
ΔE 0.0030 over the 24 patches. The profiles are 444 and 456 bytes, against Apple's 536.

**Display note, not a feature:** a 13.3" laptop IPS is likely 60–70% sRGB and uncalibrated. A colorimeter (~$150 USD) improves output more than any code here. It does **not** become an app feature — no soft-proof mode, no gamut overlay. Calibrate the display, trust the pipeline, edit the picture.

---

## 5. Pipeline — *contract frozen, v1 ordering provisional*

Order is not cosmetic. Exposure before curve is a different image than curve before exposure. What's frozen is that order is **explicit, declared and versioned**. The specific v1 sequence below is a hypothesis to be validated against golden images (§12.1), not scripture.

```
pipeline_version: 1        ← PROVISIONAL ORDERING

  —— RAW prefix (RAW inputs only, no user controls) ——
  A  demosaic
  B  camera matrix + highlight recovery
  C  lens correction

  —— image preamble (once, all inputs) ——
  0  colourspace normalise → working space
  1  geometry: perspective, rotate, crop

  —— layer body (once per entry in `stack`, in document order) ——
  2  white balance (temperature / tint)
  3  exposure
  4  highlights / shadows / blacks
  5  contrast
  6  tone curve — luma, then per-channel RGB
  7  HSL / colour mixer
  8  colour grading wheels
  9  vibrance / saturation
 10  detail: sharpen, texture, clarity      (spatial, halo 32)
 11  noise reduction                        (spatial, halo 32)
 12  effects: vignette, grain               (spatial, halo 16)
     ── result composited over the layer's input, through its mask, at its opacity ──

  —— output (once) ——
 13  display or export encode
```

**Stages 2–12 are the loop body; the stack is the loop.** There is no separate "global" pass. A global adjustment is a layer with `mask: null` sitting first, running exactly the same stages as every other layer. **One code path** — and this is worth stating precisely, because the previous revision of this section listed per-layer application as a *stage* after stages 2–12, which describes a second, incompatible execution model. It doesn't exist. A `mask: null` layer composites at full coverage, so its mask multiply and composite are identity and both are skipped, the same elimination §6.2 performs on identity nodes.

Absent deliberately: view transform, filmic, sigmoid, working-profile selector.

**Reorder policy — decide this before v0.2:**

- **Before v1.0:** reordering may change previously saved edits. Accepted. The app warns on open when a document was written under an older `pipeline_version` and offers to re-render on the current one.
- **After v1.0:** reordering requires either a parameter migration that preserves appearance, or a preserved legacy path. Keeping every historical shader chain alive forever is expensive; say so now rather than discovering it.

---

## 6. Document model

### 6.1 On disk

One sidecar per image, plus a disposable cache directory.

```
photos/
├── IMG_4821.HEIC
├── IMG_4821.HEIC.photodesk.json
└── .photodesk/
    ├── proxy/…      ← regenerable
    ├── masks/…      ← regenerable
    └── embed/…      ← regenerable
```

**The sidecar keeps the photograph's whole filename, extension included.** An earlier
revision of this section dropped it — `IMG_4821.photodesk.json` — which is tidier and
which collides in the one workflow §14 gives v0.1: open a HEIF, edit, export. The
export lands as `IMG_4821.jpg` beside `IMG_4821.HEIC`, those two share a sidecar under
the shorter name, and editing the export overwrites the original's edits with no error
and nothing to notice. Keeping the extension removes the collision rather than
detecting it, and darktable settled the same question the same way.

```json
{
  "photodesk": 1,
  "pipeline_version": 1,
  "source": {
    "file": "IMG_4821.HEIC",
    "hash": "blake3:9f2a…",
    "dimensions": [4032, 3024],
    "colorspace": "display-p3",
    "orientation": 1
  },
  "geometry": { "crop": {"x":0.02,"y":0.0,"w":0.96,"h":1.0}, "rotate": -1.4 },
  "stack": [
    {
      "id": "global", "op": "adjust", "op_version": 1,
      "enabled": true, "mask": null,
      "params": { "exposure": 0.35, "contrast": -4, "highlights": -22, "shadows": 18 }
    },
    {
      "id": "l_7c31", "op": "adjust", "op_version": 1,
      "name": "Sky", "enabled": true, "opacity": 1.0,
      "mask": {
        "op": "union",
        "components": [
          { "type": "ai", "kind": "sky", "feather": 12 },
          { "type": "linear", "from": [0.5,0.0], "to": [0.5,0.42] }
        ]
      },
      "params": { "exposure": -0.4, "saturation": 8, "temperature": -300 }
    }
  ],
  "output": { "format": "jpeg", "quality": 92, "colorspace": "srgb", "metadata": "keep-minus-gps" }
}
```

**Rules:**

- `params` is a **fixed schema per `op` + `op_version`**. Unknown keys are a validation error, never a silent no-op.
- Omitted keys mean *identity*, not zero. Identity stages are skipped entirely at render time.
- A preset is this document minus `source`, `geometry`, `output`. Style presets replace `stack`; tool presets merge into it.
- History is an append-only list of document deltas, never pixel states. It is **session-only and not serialised** — a sidecar that accumulated every gesture forever would grow without bound, and undo across sessions is not a promise this app makes.
- **The schema is defined once, in Rust, and the TypeScript types are generated from
  it** (`src-tauri/src/photodesk/document/`, emitted to `src/document/generated/`).
  §13 puts the *editing* model in the front end and that is where it belongs; the
  schema is a different thing, and written down twice it is two schemas. The generated
  file is committed so a fresh clone builds, and a test rewrites it and then fails if
  the contents moved — so staleness is caught with the fix already applied.
- **`metadata` removes rather than unreferences, and never carries a thumbnail.**
  `keep-minus-gps` rebuilds the EXIF block from the entries that survive, so the
  coordinates are gone from the file rather than merely unreachable through the tag
  tree — anything that walks the segment instead of the structure would still find
  them otherwise. And IFD1's thumbnail is dropped under *every* policy: it is a picture
  of the source, so it shows a file browser the unedited photograph, and after a crop
  it hands back exactly what the crop removed.
- **No cache path is ever written into the document.** Cache locations are derived from the keys in §9.3. A document that names a file inside disposable `.photodesk/` is a document with a dangling reference, which contradicts §0's "no cache is ever load-bearing state".

### 6.2 In memory — typed edit graph

The document is stack-shaped on disk and **the UI is always a stack**. Internally it compiles to a typed, versioned DAG.

```
Node = { id, op: OpKind, op_version: u32, inputs: [NodeId], params: TypedParams, mask: Option<MaskId> }
```

**Why a graph and not just a stack:** a stack forces linearity onto operations that aren't linear. A mask is an *input to* an operation, not an operation in sequence. An AI removal produces a new image source that downstream nodes consume. A virtual copy branches. A stack fudges all three; a DAG models them.

**Discipline, so this doesn't become a node editor nobody asked for:**

- The graph has **no UI**. Ever. If a topology can't be represented in the stack UI, it isn't allowed to exist yet.
- The graph is a *build product* of the document, reconstructed on load. It is not the serialisation format.
- When the stack becomes genuinely lossy for a topology worth having, that's a `photodesk: 2` schema bump — a deliberate, migrated event, not a drift.

Benefits that arrive for free: identity-node elimination, common-subexpression caching (two layers sharing a mask compute it once), dirty-subgraph invalidation on a slider drag instead of full re-render, and a stable target for future modules.

**They arrive free from one mechanism, and it is worth naming.** A node's key is `H(what it does, the keys of its inputs)` — a Merkle hash, so it covers the whole subgraph beneath it. Deduplication is then a lookup on insertion rather than a pass afterwards, and the dirty set is key-set subtraction: recompiling after an edit gives identical keys for everything the edit did not reach. That generalises past the slider §6.2 asks for, at no extra cost — a reorder, an insertion and a deletion are answered by the same subtraction, where a positional dirty flag needs a case for each and gets one of them wrong.

Two consequences the implementation makes structural rather than conventional:

- **The key is scale-free**, because §12.2 renders one document at proxy and at full-res and compares them — a claim that only means something if both runs execute one graph. Resolution belongs to execution, and to *caching*: `Node::cache_key(w, h)` folds it in, which is §9.3's `mask_resolution` rule and its "different caches, different keys" rule at the same time.
- **Feather and invert are their own nodes**, downstream of the mask shape. §9.3 says feather must not be in the key or every nudge of the slider re-runs the segmenter; splitting them means the shape's key *cannot* contain the radius, so the rule stops being something to remember.

**Compiled once, in Rust.** §13 put "DAG compile, dirty tracking" in the front end. The preview runs in the webview and the export runs natively (§7.2), so two compilers would produce two topologies and drift the way two shader sources would — §0 froze an invariant over exactly that shape. The graph is compiled here, serialised, and executed by both; `src/graph/` is the executor.

### 6.3 Migration

| Condition | Behaviour |
|---|---|
| `photodesk` newer than app | **Reject.** Clear message. Never guess at a future schema. |
| `photodesk` older, migration exists | Migrate on load, write back on next save. |
| `photodesk` older, no migration | Open **read-only**, explain, offer export-as-new. |
| `pipeline_version` older | Open, warn that appearance may differ, offer explicit re-render on current pipeline. Never silently re-render. |
| Unknown `op` or `op_version` | Reject the document. A partially-understood edit is worse than a refused one. |
| Unknown key in `params` | Validation error. |

Migrations are pure functions, one per version step, composed, each with a test fixture in `tests/fixtures/migrations/`.

---

## 7. Render architecture

**§7.2 is settled** (Spike C, `SPIKE-C.md`): preview runs WebGL2 inside the webview from WGSL transpiled by `naga`, export runs the same WGSL natively through wgpu, and the two agree to max 0.0088 on a 196,608-sample comparison. Everything below stands as written; the provisional framing on §7.2 is gone.

### 7.1 Proxy editing

Lightroom, Capture One and darktable have all edited a screen-resolution proxy since roughly 2007. This isn't a hardware compromise; it's how the category works.

```
source ──decode once──► full-res linear
                              │ downsample
                              ▼
                    PROXY (2× viewport)
                              │
                              ▼
                    shader chain ──► display encode ──► screen

EXPORT ──► tiled full-res ──► same shader chain ──► encode
```

Proxy size `min(2 × viewport_longest_edge, source_longest_edge)` — ~2 MP at 1080p.

### 7.2 Where the shaders run — *settled, and now running*

Tauri on Linux uses a GTK/WebKit webview, and the native-wgpu-behind-the-webview approach doesn't work there — the two contend for the same surface and flicker (tauri-apps/tauri#9220).

**Candidate:** preview in the webview via WebGL2; export in Rust via wgpu; author once in WGSL and transpile to GLSL ES 3.0 with `naga` at build time. Zero per-frame IPC, and the interactive path lands in a technology already written fluently here.

**Constraint, measured (§2.1):** `naga`'s GLSL backend cannot lower compute shaders, storage textures or storage buffers to GLSL ES 3.0, because those constructs do not exist there. That killed the candidate only while the chain to be transpiled was RapidRAW's. Ours is written fragment-first (§2.3), which stays inside what GLSL ES 3.0 has. **Fallback if transpilation still fails:** hand-write the preview chain in GLSL. Roughly fifteen fragment shaders of well-understood per-pixel math — tedious, not hard. WebGPU in WebKitGTK is the eventual clean answer but not yet dependable.

**Measured through the product, 2026-09-06.** Spike C proved the *path* with a stress shader and a hand-written harness page. v0.1's front end is the product, and it is measured the same way: `tests/renderer/web/plan.html` runs **`src/graph/execute.ts` and `src/canvas/gl.ts`, the modules `main.ts` imports**, inside webkit2gtk-4.1, over inputs Rust emitted — and compares against wgpu's render of the same compiled plan. The two agree to **max 0 of 255 across 12,288 channels**: not close, identical after quantisation. All nine of the document's parameters bind by name, and the orientation parity is checked rather than assumed. Run it with `python3 tests/renderer/web/run-probe.py --engine webkit2gtk-4.1 --plan`.

**Two things about GL that the shaders' author does not have to know, because they are resolved once at the edge.** WGSL is authored for wgpu, whose framebuffer origin is **top-left**; GL's is **bottom-left**, so the same `uv` expression addresses the opposite end and *every pass inverts the image*. An odd pass count would present the photograph upside down, and the pass count changes with the number of layers — so it cannot be settled by convention. `canvas/preview.ts` draws the whole chain into offscreen targets and resolves the parity at a **blit**, which also makes fit-to-window and §11's split view free. And the plan's **output** node draws into an 8-bit target rather than an f16 one: stage 13 has encoded for the display by then, and `blitFramebuffer` refuses to copy between a floating-point read buffer and the fixed-point canvas (ES 3.0 §4.3.2). That refusal raises no error the user can see — it presents as a blank canvas.

**RapidRAW does not solve this on Linux, and its non-solution is worth knowing.** It disables its wgpu renderer on Linux outright, reads back every frame to the CPU, mozjpeg-encodes it at quality 65–85 and ships the bytes over IPC. That is the far end of the trade from "zero per-frame IPC", and it is 8-bit and lossy. Independent confirmation that the surface-contention problem is real, and that there is no free path hiding in a fork.

### 7.3 Performance budget

| Metric | Budget | Note |
|---|---|---|
| Slider drag → new frame | **16 ms** (60 fps) at proxy | Degrades to half-proxy during drag, refines on release |
| Image open → first render | < 800 ms for 12 MP HEIF | Decode may be async; UI never blank-blocks |
| Mask overlay toggle | < 50 ms | |
| Export | **never blocks the UI** | Worker thread, cancellable, progress events |
| Proxy + graph memory | ≤ 512 MB, LRU, configurable | |
| Disk cache | ≤ 4 GB, LRU, configurable | On external drive |
| Undo depth | ≥ 100 gestures | Deltas, not pixels — cheap |

**60 fps is an architecture requirement, not a wish.** Fifteen discrete render passes at 2 MP f16 means ~15 × 32 MB of texture round-trips per frame — roughly half a gigabyte of traffic, which no amount of ALU saves you from. **The per-pixel stages (2–9) must be fused into a single pass.** Only spatial stages (10–12) need their own. That's four or five passes total, and then the budget is comfortable.

**Four or five passes is per layer, not per frame.** §5 makes the stack the loop, so six mask layers means six times that traffic, and the budget silently becomes a different problem. State it as a bound and test it: **60 fps at proxy with up to 6 layers**, degrading to half-proxy during drag as above. Beyond 6, frame rate is allowed to fall and the UI says so rather than pretending. If a future stage can't be fused, that's a design review, not a quiet extra pass.

---

## 8. Masks

**Components:** `brush`, `linear`, `radial`, `luminance`, `color`, `ai(kind)`.
**Composition:** `union | intersect | subtract` in declared order, each with independent feather and invert.

- Parametric components store parameters, rasterise on GPU each frame. Free.
- Brush stores vector stroke paths, rasterised on GPU — resolution-independent, so proxy and export agree.
- AI components cache a bitmap (§9.3).

**Evaluation point — `luminance` and `color` components read the layer's input, not the source and not the final image.** These two are the only components computed *from pixels*, so the pixel state they read has to be named or it will be chosen accidentally and differently in the preview and the export. "Layer input" means the image as it stands after every earlier layer in the stack has been composited, which follows from §5's loop model and makes a luminance mask behave the way stacking a second adjustment on top of a first already behaves. The consequence to accept knowingly: raising exposure on layer 1 moves what layer 2's luminance mask selects. That is the correct behaviour and it is also the surprising one, so the UI shows the mask live while it is being edited.

If an AI mask can't be computed, the layer renders using its non-AI components only and is **flagged in the UI**. It never silently renders something wrong.

---

## 9. AI — backend-agnostic

**Frozen:** the provider interface, and that AI is removable without touching the render path.
**Provisional:** which providers are implemented, and the routing policy.

### 9.1 Provider interface

```
trait Provider {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> CapabilitySet;   // {segment, inpaint, denoise, upscale, ocr}
    fn health(&self) -> Health;
    fn execute(&self, task: Task) -> Result<Artifact>;
}
```

`execute` and `health` are synchronous signatures, and every provider is invoked **on a worker pool, never on the UI thread** — that is what keeps §11's "AI runs async; the image stays interactive" true without infecting the trait with async. `health()` returns a cached value refreshed on a timer; a provider that probes the network inside `health()` will stall the panel that draws its status.

Four interchangeable implementations, all equal citizens:

```
LocalCpu      — ONNX Runtime, CPU EP
LocalGpu      — ONNX Runtime, GPU EP (ROCm / Vulkan / CUDA, whatever exists)
RemoteHttp    — generic HTTP endpoint or API gateway
ComfyUI       — workflow-graph backend, local or over the network
```

**Routing is configuration, not code.** A per-capability preference list with fallback, in a config file the user edits:

```toml
[routing]
segment = ["LocalCpu", "RemoteHttp"]
inpaint = ["ComfyUI", "RemoteHttp"]
denoise = ["LocalGpu", "LocalCpu", "RemoteHttp"]
upscale = ["LocalGpu", "RemoteHttp"]
ocr     = ["RemoteHttp", "LocalCpu"]
```

The current hardware makes remote the sensible *default policy* for heavy generative work. That's a config file, not an architecture. Better hardware changes one line.

**Ship discipline:** interfaces defined now, `LocalCpu` and `RemoteHttp` implemented at v0.4, the rest returning `Unavailable`. Abstraction layers are how solo projects die — the interface is cheap, ten implementations are not.

### 9.2 Segmentation

```
trait Segmenter {
    fn id(&self) -> ModelId;
    fn version(&self) -> ModelVersion;
    fn prepare(&self, image: &Image) -> Result<Session>;      // may be a no-op
    fn segment(&self, session: &Session, prompts: &Prompts) -> Result<Mask>;
}
```

The two-phase split exists because SAM's encoder runs **once per image** with a cached embedding, after which each click hits only the lightweight decoder in milliseconds. Single-phase models implement `prepare` as a no-op and lose nothing. That generalisation is why this is an interface and not a SAM binding.

SAM 2.1 Hiera Tiny is the **initial implementation**, not a dependency. `darktable-ai` publishes statically-shaped ONNX models with a documented interface and declared tile sizes — consume that catalogue rather than building one.

### 9.3 Cache keys

Under-specifying this produces stale masks that look like rendering bugs and cost a weekend each.

```
mask_cache_key = H( source_hash
                  , operation
                  , provider_id
                  , model_id
                  , model_version
                  , mask_resolution   // the bitmap's own pixel dimensions
                  , config_hash       // tile size, thresholds, refinement passes, precision
                  , input_state_hash ) // prompt points, their order, mask kind
```

**Feather is deliberately not in the key.** Feather is a cheap blur applied to the cached bitmap on the way out; putting it in `input_state_hash` would make every nudge of the feather slider invalidate the embedding and re-run the segmenter, turning a free control into a multi-second one. Anything applied *after* the cached artefact stays out of the key by the same argument.

**`mask_resolution` is in the key** because the alternative is a proxy-resolution mask silently serving a full-resolution export — a soft edge nobody ordered, appearing only in the exported file, which is the worst place to find it. With resolution in the key, proxy and export are separate entries and the miss is explicit.

Different caches, different keys — don't share one scheme:

```
proxy_cache_key = H( source_hash, proxy_dimensions, working_colorspace, pipeline_version )
embed_cache_key = H( source_hash, model_id, model_version, config_hash )
```

Note `pipeline_version` belongs in the proxy key and **not** in the mask key: masks derive from the source, not from the pipeline. Every key is written into the cache entry's header so a mismatch is detectable rather than assumed.

### 9.4 Failure

Provider down, model missing, endpoint unreachable → the capability greys out with a reason, and nothing else changes. Tested by deleting the sidecar binary: the app must still open, edit and export.

---

## 10. Visual system

**Identity:** PhotoDesk stands alone. No Beben Design branding in the application.

### 10.1 Monochrome chrome, semantic colour

**The rule: the interface is monochrome. Colour appears only where colour is the information.**

Permitted — colour *is* the data:
- Clipping warnings (highlights and shadows)
- Histogram channels; RGB curve channel indicators
- Colour grading wheels, HSL channel selectors, white balance indication
- Any control whose subject is hue

Forbidden — colour as decoration or state:
- Accent colours, brand colours, status colours, hover/focus/selection tints
- Anything in the header bar, tab strip, or **adjacent to the image canvas**

The second constraint is the technical one: hue next to a photograph shifts how you perceive that photograph's colour. Colour-carrying controls live inside panels, away from the canvas. A grading wheel is fine; a green "saved" toast beside the image is not.

| Token | Value | Use |
|---|---|---|
| `canvas` | `#333333` | surround around the image |
| `surface` | `#1E1E1E` | panels, sidebars |
| `surface-raised` | `#252525` | popovers, menus, active panel |
| `border` | `#3A3A3A` | dividers, control tracks |
| `text` | `#F2F2F2` | primary labels, numerics |
| `text-dim` | `#8C8C8C` | secondary, units, inactive |
| `text-mute` | `#5A5A5A` | disabled |
| `focus` | `#FFFFFF` | 2px focus ring |
| `warn` | `#E71D36` | warnings and clipping only |

**Canvas is `#333333`, not black.** A near-black surround makes images read brighter and more contrasty than they are, and you'll systematically under-expose. ~18–20% reflectance is the reference-viewing convention and it's still monochrome.

**State without hue:** selected → `surface-raised` + 1px border lift · active → `text` vs `text-dim` at rest · focus → 2px white ring · hover → +6% surface luminance · disabled → `text-mute`, no opacity tricks.

**Mask overlay:** masked regions as white at 35% with a 1px white outline; a modifier inverts luminance on the *unmasked* side instead. Both read on any photograph, which a red overlay does not.

### 10.2 Type

- **UI:** Adwaita Sans — the GNOME system font, so it is already on the target platform with nothing to download or bundle. Inter as fallback.
- **Numerics:** JetBrains Mono, tabular figures. `+0.35 EV`, `5200 K`, `−12`, `ƒ/2.8` must not shift the layout while dragging.
- **No serifs anywhere.**

### 10.3 Window

```
┌──────────────────────────────────────────────────────────┐
│  ‹ Photos          IMG_4821.HEIC              Export     │
├──────────────────────────────────────────────────────────┤
│                    the photograph                        │
│                  (#333333 surround)                      │
├──────────────────────────────────────────────────────────┤
│  Crop   Light   Color   Detail   Effects   Masks   AI     │
├──────────────────────────────────────────────────────────┤
│  Exposure        ──────────●───────────      +0.35 EV    │
│  Contrast        ────────●─────────────          −4      │
│                                    ▾ Advanced            │
└──────────────────────────────────────────────────────────┘
```

Four to six controls per tab; `Advanced` reveals the rest. GNOME conventions where free: header bar, `prefers-color-scheme`, `Esc` to dismiss. Dark is the default and the one designed properly.

**No framework.** TypeScript and Vite, zero runtime dependencies. Two reasons and one accepted cost.

The canvas does not need one — Spike C's harness runs the full stack at 2 MP as plain ES modules with no build step, because a WebGL2 context, a uniform buffer and three draw calls touch no framework API. And §11 specifies the interaction surface down to `Shift`-drag being 0.1× travel, double-click-to-reset, scroll-only-when-hovered and one-gesture-one-undo-entry; a component library's slider does none of that, so it would be overridden rather than used, and overriding a control is more work than writing one.

**The cost, stated so it is not a surprise:** panels, undo, focus management and the keymap are all hand-written, and none of that bill falls due at v0.1 — it arrives across v0.2 to v0.7. The reason to accept it is that this is an application of exactly one screen with roughly forty controls on it, for an audience of one, that wants to still build in a decade.

---

## 11. Interaction principles

These are product surface, so they're specified, not left to the implementation.

### Sliders

| Gesture | Behaviour |
|---|---|
| Drag | Continuous update, never blocks. Degrades resolution rather than dropping frames. |
| `Shift` + drag | Fine — 0.1× travel |
| `Ctrl` + drag | Coarse — 10× travel |
| Double-click label or track | Reset to default |
| Click the value | Numeric entry, `Enter` commits, `Esc` cancels |
| `↑` / `↓` | One step; with `Shift`, one fine step |
| Scroll over control | Adjust — only when the control is hovered, never when the panel is |

**One completed gesture is one undo entry.** A slider drag is a single history record, not two hundred. History commits on pointer-up or on numeric commit.

### Global keys

| Key | Action |
|---|---|
| `Space` (hold) | Show original. Release returns. Never a toggle. |
| `\` | Toggle before/after split view |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Ctrl+E` | Export |
| `Ctrl+C` / `Ctrl+V` | Copy / paste edit |
| `F` | Fit to window |
| `1` | 100% — canvas focus only |
| `0`–`5` | Rating — filmstrip and grid only, never canvas (v0.7) |
| `Esc` | Dismiss overlay, cancel gesture, exit mask edit |

### Non-negotiables

- No modal progress dialogs. Long operations run inline with cancel.
- No operation blocks the canvas. AI runs async; the image stays interactive.
- Every destructive action is undoable or confirmed. Never both, never neither.
- Reset-to-default is reachable in one gesture from any control.

---

## 12. Testing

The regression suite is not optional infrastructure — for a non-destructive editor it *is* the correctness argument.

### 12.0 Standing suites

Two permanent suites came out of Phase 0 (§13), neither of which is a spike artefact:

- **`tests/color/`** — Spike B's harness. Two of its tests cross-validate against lcms2 rather than the pipeline against the harness, because a colour suite that only agrees with itself is green and meaningless. **It now tests the product rather than a copy of it**: the spaces, curves, matrices and gamut policy moved into `engine/` when the decode path needed them, and what stays here is measurement — ΔE2000, Lab, the corpus, and a model of the pipeline whose precision is a parameter. Before the move the suite validated its *own* transforms, which is a weaker claim than it looks: it could have been green while the shipped ones were wrong, there being none.
- **`tests/renderer/`** — Spike C's harness. Six tests covering WGSL→GLSL lowering, the negative control that compute is refused, the UBO size bound, and a wgpu-rendered reference. The WebKitGTK half needs a browser and so runs on demand rather than in CI.

#### The colour suite

Spike B's harness (`tests/color/`) is a permanent suite per §13, not a spike artefact. Eight tests, ~20 ms, no fixtures — it runs on every commit. Two of its tests cross-validate the harness against lcms2 rather than the pipeline against the harness, because a colour suite that only agrees with itself is green and meaningless.

### 12.1 Golden images

A committed corpus of ~12 sources × ~8 documents, rendered and compared to blessed reference PNGs by ΔE2000 (max and mean thresholds per case).

That is ~96 reference images, so fix the storage question **before the first `--bless`, not after**: references render at proxy resolution (~2 MP), 16-bit PNG, and both the source corpus and the references live in **git-lfs**. Retrofitting lfs onto a repository that already has a hundred binaries in its history is a rewrite of that history.

**Blessing workflow — the part that's usually skipped and then poisons the suite:** intentional pipeline changes will fail these tests by design. `cargo test --bless` regenerates references, but the diff must be reviewed **visually** in a side-by-side report, and the blessing is a separate commit citing the change that caused it. Blind blessing turns a regression suite into a rubber stamp within a month.

### 12.2 Proxy / full-res agreement

Render the same document at proxy and at full-res-downsampled-to-proxy. Assert they match within threshold.

This is the test that enforces the one-shader-source invariant. Without it the invariant is a comment.

**Measured at v0.1, before there are any spatial stages, and the number was not the expected one.** With `adjust` at `op_version` 1 the chain is per-pixel only, so the naive expectation is near-exact agreement. It is not, and two wrong diagnoses were reached before the right one — the first attributed it to the adjustment chain, the second to clipping at the rails, and the disagreement turned out to sit *away* from the rails with no adjustment applied at all.

**It is the gamut map.** Proxy editing computes `f(mean(pixels))` where export computes `mean(f(pixels))`, and §16 #11's constant-luminance chroma clip is emphatically not linear: two neighbouring pixels, one outside sRGB and one inside, are worth about 18 codes of disagreement on their own. Every gamut policy has this, the per-channel clip included.

So the threshold has to be stated against a **precondition**, not as a number:

| content | worst | mean |
|---|---|---|
| band-limited **and** in-gamut | **0.1486** | 0.0355 |
| detail finer than the proxy, full chain | 152.53 | 11.31 |
| the same detail, stage 13 alone | 107.86 | 3.27 |

*8-bit codes, 256² reduced to 64².* Under both preconditions — no detail the reduction loses, nothing outside the destination gamut — `mean` is the identity and the chain is smooth, so the two paths have no way to disagree and they do not: 0.15 of a code is the RGBA16F attachment's own truncation. That is where §12.2's bar belongs, and a spatial stage that broke it would show up there first.

The rest is inherent to proxy editing rather than a defect, every editor since 2007 has it, and it is **not a lie the user can see**: §7.1 sizes the proxy at twice the viewport, so detail the proxy cannot hold is detail the screen cannot show, and zooming in makes the proxy finer.

**Spatial stages need a stated policy or this test fails by construction.** Stages 10–12 are resolution-dependent by nature: a 32-pixel sharpening halo covers a different fraction of a 2 MP proxy than of a 12 MP source, and grain rendered at full resolution then downsampled averages away to nothing while grain rendered at proxy stays visible. Left unaddressed, the test goes red on day one, gets muted in week three, and §12.2's own closing sentence comes true. So:

| Rule | Consequence |
|---|---|
| **Spatial radii are declared in source-image-relative units** and multiplied by the render scale at dispatch | Sharpen, clarity, texture, noise reduction and vignette agree between proxy and export |
| **Grain is seeded deterministically from `(source_hash, layer_id)` and its cell size scales with render scale** | Grain is reproducible and the same size on screen as in the file |
| **Grain is excluded from the §12.2 comparison, and only grain** | Scale-correct grain is still not downsample-invariant. This is a carve-out with a reason, recorded here, not a silently loosened threshold |

The carve-out is tested from the other side: a document with grain enabled must produce *identical* output on two runs at the same resolution, which catches the seeding bug that the agreement test can no longer see.

### 12.3 Source preservation

```
for each source in corpus:
    hash_before = blake3(file)
    open → apply document → render preview → export → close
    assert blake3(file) == hash_before
```

Byte-identical. Not "metadata unchanged" — identical. Runs in CI on every commit. This is invariant #1 and it's the cheapest possible test for the most expensive possible bug.

**Running with all five steps since 2026-09-06** (`src-tauri/tests/export.rs`). The mtime is asserted alongside the hash, because a rewrite with identical bytes is still a rewrite and it is the kind that survives a hash comparison — and the test carries its own counterexample, so a hash that cannot see a change is not mistaken for a test that passed.

### 12.4 Others

- **Migration:** every version step has a fixture pair, plus rejection tests for newer schemas and unknown ops.
- **Cache invalidation:** mutate each key component in turn, assert the cache misses.
- **AI-absent:** delete the provider binaries, run the full corpus, assert open/edit/export all succeed.
- **Performance:** assert the §7.3 budgets on the reference machine. Failures are warnings, not build breaks — but they're recorded per commit so drift is visible.

---

## 13. Repository

```
photodesk/
├── src/                        ← front end (TypeScript + Vite, no framework)
│   ├── document/               ← editing model, history, presets
│   │   └── generated/          ← the schema's TypeScript types. Emitted from Rust,
│   │                             committed, staleness caught by a test (§0 register)
│   ├── graph/                  ← executes the compiled plan (no UI). The *compile*
│   │                             is in Rust — see §6.2, and the §0 register
│   ├── panels/                 ← Crop Light Color Detail Effects Masks AI. v0.1 is Light;
│   │                             the rest are drawn, disabled, and say which version
│   ├── canvas/                 ← viewport, preview renderer, before/after
│   ├── design/                 ← tokens, type scale
│   └── ipc.ts                  ← the Rust boundary, in one file and deliberately short
├── app/                        ← the window. `tauri`, `main.rs`, the IPC commands and
│   │                             nothing else — a separate crate so that `photodesk`
│   │                             stays linkable without webkit (§0 register)
│   ├── src/main.rs             ← what crosses the boundary, and what deliberately does not
│   ├── capabilities/           ← what the one window may ask the core for
│   └── tauri.conf.json
├── src-tauri/src/              ← the library. **No `tauri` dependency, ever**, so §12.3
│   │                             and §12.1 can run headless on every commit
│   ├── engine/                 ← our render core: decode, colour, GPU dispatch, shaders
│   │   ├── colour.rs           ← spaces, transfer curves, matrices — all derived
│   │   ├── icc.rs              ← reading an embedded profile; §4's two spaces or an error
│   │   ├── decode.rs           ← §5 stage 0: a file becomes linear P3 f16
│   ├── glsl.rs             ← §7.2's preview half: WGSL → GLSL ES 3.00, at startup
│   │   ├── gamut.rs            ← §16 #11's export policy
│   │   ├── image.rs            ← the working buffer, and §7.1's proxy resample
│   │   ├── render.rs           ← executes a compiled graph through wgpu (§7.2)
│   │   ├── exif.rs             ← what a photograph says about itself, and §6.1's policy
│   │   └── export.rs           ← §6.1's `output` block: a frame becomes a file
│   ├── photodesk/              ← document → engine bridge, IO, cache, export
│   │   ├── document/           ← the schema: model, validation, migration
│   │   ├── graph/              ← §6.2's DAG compile and dirty tracking
│   │   └── sidecar.rs          ← where it sits on disk, and how it is written
│   └── ai/                     ← provider registry, routing
├── shaders/photodesk/          ← WGSL source of truth. The only shader source (Spike A gate failed)
│   ├── adjust.wgsl             ← §5 stages 2–9 for one layer, fused (§7.3)
│   └── encode.wgsl             ← §5 stage 13, with §16 #11's gamut policy
├── providers/                  ← LocalCpu, RemoteHttp, ComfyUI, LocalGpu
├── tests/
│   ├── golden/                 ← corpus + blessed references
│   ├── color/                  ← Spike B harness, kept as a permanent suite
│   ├── renderer/               ← Spike C harness, likewise. Holds the WGSL↔reference
│   │                             agreement §12.2 inherits, and stage 13's (§16 #11)
│   └── fixtures/migrations/
├── docs/{ARCHITECTURE,FORK-AUDIT,PIPELINE,DOCUMENT,DECISIONS}.md
└── packaging/rpm/
```

**`src-tauri/` links no webview yet, and that is a decision rather than a delay.** §14
gives v0.1 a document model, a graph compile, source-preservation and golden tests
before it gives it a window, and every one of those has to run headless — which is
criterion 1 of the gate §2.1 failed RapidRAW on. A crate that links webkit cannot run
them, so `tauri` and a `main.rs` arrive when there is a window, and everything before
that stays testable without one. It is also how a Tauri v2 project is laid out anyway.

**There is exactly one shader source, `shaders/photodesk/`, and it is ours.** Spike A's gate failed, so nothing is vendored and `engine/` is our own render core rather than somebody else's, read-write like the rest of the tree. Two live shader sources is the WYSIWYG drift §0 freezes against, wearing a directory layout as a disguise; the ambiguity that made that possible is gone.

`DECISIONS.md` is an append-only log: each entry records what moved from PROVISIONAL to FROZEN, the spike that resolved it, and the date. The §0 register is the current state; this is the history.

**Packaging is one RPM.** No AppImage, Flatpak or DEB. A build matrix for an audience of one is the mad lab wearing a different hat.

**That RPM cannot satisfy its own most important dependency, and the spec should say so rather than let the first install discover it.** §1's native subject is an iPhone HEIC, which is HEVC-coded; Fedora's `libheif` ships without HEVC on patent grounds (§3); the codec lives in RPM Fusion's `libheif-freeworld`, which is a third-party repository a Fedora package may not require. Three options, none of them free:

| Option | Cost |
|---|---|
| `Requires: libheif-freeworld` and document that RPM Fusion must be enabled | Honest, and the package simply will not install without it |
| `Recommends:` it, and detect the missing codec at runtime with a clear message | The app installs and opens JPEG, PNG and AVIF; HEIC fails with an explanation and an install command rather than a decode error |
| Bundle a decoder | Ships an encumbered codec inside the RPM. Not doing this |

The second is the one that matches §9.4's existing posture — a missing capability greys out with a reason and nothing else changes — and it is the only one where the app is still useful on a stock install. **Decide it before v0.7 packaging; note it now so v0.1's decode path returns a distinguishable "no codec" error rather than a generic failure.**

---

## 14. Roadmap

| Version | Scope | Estimate |
|---|---|---|
| ~~**0.0**~~ | ~~Spikes A, B, C.~~ **Complete 2026-09-05.** `FORK-AUDIT.md` ✅ · `SPIKE-B.md` ✅ · `SPIKE-C.md` ✅ | 3 weeks est., 1 day actual |
| **0.1** | Open iPhone HEIF → exposure, contrast, highlights, shadows, blacks, temperature → before/after → export → colour correct end to end. Document model, graph compile, source-preservation and golden tests running. | 3 weeks |
| **0.2** | Crop, rotate, straighten. Presets, copy/paste edits. Undo/redo at gesture granularity. | 2 weeks |
| **0.3** | Colour tab: curves, HSL, grading wheels, vibrance. | 3 weeks |
| **0.4** | Masks: brush, linear, radial, luminance, colour range, composition. | 4 weeks |
| **0.5** | AI: `Segmenter` + `LocalCpu`; `RemoteHttp` for inpaint, denoise, upscale, OCR. | 3 weeks |
| **0.6** | RAW prefix. | 1–2 weeks |
| **0.7** | Workflow: folders, ratings, search, batch export. | 4 weeks |

**v0.1 deliberately excludes curves and HSL.** They're the fun part, which is exactly why they get deferred — the boring parts (colour correctness, the document model, the test harness) are the ones that are expensive to retrofit and impossible to bolt on later.

**Ship v0.1 before designing v0.4.**

---

## 15. Kill criteria

- Spike A gate fails and the from-scratch estimate exceeds the appetite → stop, contribute upstream, reclaim the weekends.
- After v0.1 you reach for RapidRAW instead of PhotoDesk → the differentiation wasn't real.
- The fork stops rebasing cleanly across two upstream releases → §2.1's boundary was violated. Fix it or accept the maintenance cost knowingly.
- A feature needs a paragraph of colour science to justify → that's the lab talking. Refuse it.
- Six weeks, no commits → this is a design exercise. Allowed, but name it and take the time back.

---

## 16. Open decisions

| # | Decision | Resolved by |
|---|---|---|
| ~~1~~ | ~~Fork or build from scratch~~ | **Closed 2026-09-05 — build. `FORK-AUDIT.md`** |
| ~~2~~ | ~~Working space and precision~~ | **Closed 2026-09-05 — linear Display P3 f16. `SPIKE-B.md`** |
| ~~3~~ | ~~Preview renderer path~~ | **Closed 2026-09-05 — WebGL2 + naga transpilation. `SPIKE-C.md`** |
| ~~11~~ | ~~Export gamut-mapping policy~~ | **Closed 2026-09-06 — clip chroma at constant luminance. §4, `tests/color/tests/gamut_policy.rs`** |
| ~~12~~ | ~~ICC extraction from real containers~~ | **Closed 2026-09-06 — proven against a real container. `tests/color/tests/heif_icc.rs`** |
| 13 | How the RPM handles HEVC — hard `Requires`, `Recommends` + runtime detection, or bundling | Before v0.7 packaging (§13). **The decode half is done**: the decoder consults libheif's codec list before it reads, so a missing codec is `MissingCodec` naming `libheif-freeworld` rather than a generic read failure — a truncated file and an unsupported one are now distinguishable. What remains is the packaging clause itself |
| 18 | Tiled full-res export (§7.1). The exporter writes whole images; the renderer releases a node's texture as soon as its last consumer has run, so a 12 MP export is a few hundred megabytes against §7.3's 512 MB cap — comfortable, and not the streaming path §7.1 describes | Before a 60 MP source, or before v0.7's batch export makes the peak matter |
| 19 | HEIF **output**, and with it EXIF for HEIF sources. §6.1's formats are JPEG, PNG and TIFF; a HEIC in, HEIC out round trip is not among them, and the metadata policy reads EXIF from JPEG only | Whenever a HEIF export is actually wanted; §4's default output is sRGB JPEG for a reason |
| ~~17~~ | ~~PNG, and therefore screenshots~~ | **Closed 2026-09-06 — decoded. §4, `engine/decode.rs`, `tests/decode.rs`** |
| 14 | Golden-image thresholds for HEIC sources, which must clear the ~0.9 ΔE YCbCr floor (§3) | Before the first `--bless` (§12.1) |
| 16 | Parameter ranges — the document validates finiteness but no bounds, so a `exposure: 400` is a legal document. §11 puts slider travel in the UI; whether the *file* has an opinion is unstated. **The UI half now exists**: `src/panels/light.ts` carries the travel for all six of v0.1's controls, and it is travel rather than validation — a document may legally carry `exposure: 400` and this build renders it, it simply cannot be dragged there | Before v0.2's presets (§6.1), which is the first thing that writes params the UI did not |
| 15 | Golden-image thresholds must also allow one colour-attachment step: an RGBA16F attachment can truncate toward zero rather than round (§0 register), which is a full-step bias on every stored channel and not the shader's doing | Before the first `--bless` (§12.1); same conversation as #14 |
| 4 | Pipeline v1 ordering, **and the formulations inside it** — the six sliders v0.1 ships each have a stated shape (`shaders/photodesk/adjust.wgsl`) and several are choices rather than facts: contrast is a gain about 18% grey in linear light, stages 4's three controls are weighted on *perceptual* rather than linear luminance, vibrance measures existing chroma relative to brightness | Golden-image validation |
| 5 | Pre-1.0 vs post-1.0 reorder policy | Before v0.2 (§5) |
| 6 | Remote AI endpoint: self-hosted ComfyUI or gateway | Before v0.5 |
| 7 | Icon — `PD` monogram or geometric mark, monochrome, no aperture. `app/icons/icon.png` is a **placeholder** that commits to neither, present because `tauri-build` requires a file | Whenever; the 16×16 render is the only test |
| ~~8~~ | ~~Front-end framework~~ | **Closed 2026-09-06 — none. TypeScript + Vite, zero runtime dependencies (§10.3)** |
| 9 | HDR gain map handling beyond v1's discard | An HDR display, or the first wanted gain-mapped export (§4) |
| 10 | Layer count at which frame rate is allowed to fall | Measured against the §7.3 bound of 6 |

**Licensing:** RapidRAW is AGPL-3.0. A licensing review is required before any redistribution, publication or portfolio use.

That review was sequenced to gate Spike A's conclusion rather than shipping, because the fork decision commits months of work and this repository is public. **The engineering gate failed first, so the review is off the critical path for that decision** — nothing is being vendored, adapted or redistributed, because there is no fork. Two things stay true: `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes and copies no source, and "architectural reference" means reading their code and then writing ours, which is a distinct question worth raising if a review still happens.

**The specification-only constraint lifts.** It existed because the fork decision was live and this repository is public. The decision is closed and the code that follows is original.

The review itself is out of scope for this document and is not an engineering decision. Nothing here should be read as legal advice.
