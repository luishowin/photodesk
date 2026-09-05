# Spike A — RapidRAW fork audit

**Date:** 2026-09-05
**Subject:** RapidRAW 1.6.3, AGPL-3.0, unpacked source (`src-tauri/` + `src/`)
**Method:** static read of the unpacked tree. Nothing was built, run, or copied into this repository.
**Deliverable for:** `ARCHITECTURE.md` §2.1

---

## Verdict

**The gate fails. Do not fork.**

Criterion 4 fails outright and criterion 3 fails on two of its three named subsystems. Per §2.1, the fork proceeds only if all four hold, so one failure would be enough; there are two, and they are structural rather than incidental.

The one-sentence reason: **RapidRAW's entire per-pixel chain is a single 1,910-line compute kernel whose stage order is the literal statement order inside one `main()` function, and §13 declares vendored `engine/` read-only — so "pipeline order is explicit and versioned", a FROZEN item, is unimplementable inside the fork.**

Three further findings, each independently large, are recorded below. Any one of them would have made the fork expensive; together they describe an application built to a different specification than this one.

The fallback named in §2.1 applies: **use RapidRAW as an architectural reference and build against `rawler` + `libheif` + our own shaders.** §7 has more to say about what that costs than it did this morning, and most of the news is good — see [What this changes](#what-this-changes).

---

## The four gate criteria

### 1. Headless invocation — **PASS**

The processing core has a real seam, and it is cleaner than §2.1 feared.

`GpuProcessor::new(context, max_w, max_h)` and `GpuProcessor::run(&self, input_view, w, h, RenderRequest, skip_cpu_readback, output_to_display)` (`gpu_processing.rs:569`, `:1100`) mention no Tauri type. `RenderRequest` (`:24`) is `{ adjustments: AllAdjustments, mask_bitmaps: &[ImageBuffer<Luma<u8>>], lut: Option<Arc<Lut>>, roi: Option<Roi> }` — all plain types.

Tauri appears only in the wrappers above it. `process_and_get_dynamic_image(context, state: &tauri::State<AppState>, …)` (`:1602`) takes `State` solely to reach a cached `GpuProcessor` behind a mutex; that is a caching concern, not a functional dependency. `get_or_init_gpu_context` (`:138`) takes an `AppHandle` that is `_`-prefixed and, on Linux, entirely unused — the branch that needs it is behind `#[cfg(not(any(target_os = "android", target_os = "linux")))]`.

A test binary could construct a `wgpu` device itself, build a `GpuProcessor`, and call `run`. This criterion passes on the merits.

It is the only one that does.

### 2. Edit-state adapter — **PASS as written, and the pass is worth less than it looks**

There is exactly one funnel: `get_all_adjustments_from_json(&serde_json::Value, is_raw, tonemapper_override) -> AllAdjustments` (`image_processing.rs:2514`), called from 16 sites across 7 modules. `AllAdjustments` is `#[repr(C)]` + `bytemuck::Pod`. Substituting `photodesk_document_to_all_adjustments(&Document) -> AllAdjustments` is mechanically trivial and touches no WGSL. **The criterion, read literally, passes.**

It should not be scored as a pass, because the thing it exists to protect does not survive.

`AllAdjustments` is not an edit state that happens to have a different shape from ours. It is a **flat parameter block**: one `GlobalAdjustments`, plus `mask_adjustments: array<MaskAdjustments, 32>` and a `mask_count` (`shader.wgsl:172`, `image_processing.rs:1617`). The shader consumes it by *summing masked parameters into scalars before running the pipeline once*:

```
for i in 0..mask_count:
    influence = mask_texture[i][coord]
    t_exposure += m.exposure * influence
    t_contrast += m.contrast * influence
    …                                        (shader.wgsl:1677–1716)
```

That is **parameter blending**. §5 specifies **pixel compositing**: "stages 2–12 are the loop body; the stack is the loop", each layer's result "composited over the layer's input, through its mask, at its opacity". These produce different images for every non-linear stage, and the difference is not a threshold — it is a different definition of what a layer is.

Three consequences follow, and none is recoverable through an adapter:

- **`opacity` cannot be expressed.** There is no composite to attenuate.
- **§8's frozen invariant has no referent.** "Masks read the layer's input" presumes a layer input. Here masks are influence scalars sampled from a pre-rasterised `texture_2d_array` at the source coordinate; there is no per-layer pixel state for a `luminance` or `color` component to read.
- **The stack is not ordered.** Summation is commutative. A PhotoDesk document's layer order carries meaning that the target struct discards by construction.

So the adapter direction that compiles — ours → theirs — is lossy at the document level, and the direction that would not be lossy requires changing the storage-buffer layout, which means editing the shader, which criterion 2 forbids.

**This is the same gate-design bug the 2026-09-05 review found in the original criterion 2**, reappearing one level down: the criterion can pass while the invariant it was written to protect fails. Recorded as a recommendation below rather than silently rescored.

### 3. RAW decode, demosaic, core shader chain all `KEEP` or `ADAPT` — **FAIL (1 of 3)**

| Subsystem | Class | Why |
|---|---|---|
| RAW decode | `ADAPT` | 257 lines of thin wrapper over `rawler` (`raw_processing.rs`). Zero Tauri references. The value is in `rawler`, not in the wrapper — and `rawler` is available without forking anything. |
| Demosaic | `ADAPT` | Not RapidRAW's code at all. `rawler::imgop::develop::RawDevelop` with `DemosaicAlgorithm::Speed` for fast mode (`raw_processing.rs:105–117`). Same observation: the asset is the library. |
| **Core shader chain** | **`REPLACE`** | Below. |

Two of three classify acceptably, and both do so because the code being classified is a wrapper around a crate this project can depend on directly. The criterion requires all three.

### 4. Stage order expressible without editing vendored shaders — **FAIL**

This is the finding that decides the spike, and it is unambiguous.

`shader.wgsl` has **one** `@compute` entry point, at line 1616, running to line 1910. Inside it, the pipeline is a straight-line sequence of calls:

```
noise reduction → sharpen → clarity → structure → centre local contrast
  → exposure → glow → halation → flare → dehaze → white balance
  → centre tonal → filmic brightness → tonal (contrast/shadows/whites/blacks)
  → highlights → colour calibration → HSL → hue shift → vibrance/saturation
  → colour grading → per-mask grading mix → vignette
  → TONEMAP TO sRGB
  → curves → per-mask curve mix → LUT → grain → clipping warnings → dither
  → textureStore(rgba8unorm)
```

There is no ordering parameter. A search of the file for `order`, `sequence`, `stage`, `pass_index` and `pipeline` returns nothing. The order *is* the statement order. To change it you edit `shader.wgsl`, and §13 says `engine/` is never edited.

It is also not the order PhotoDesk wants, and not by a permutation:

- **Noise reduction and sharpening run first, before exposure.** §5 puts them at stages 10–11, after all tonal work.
- **White balance runs after exposure.** §5 puts it at stage 2, before.
- **Curves run *after* the display encode**, on sRGB-encoded values (`:1829–1840` tonemap, `:1853` `apply_all_curves`). §5 puts the tone curve at stage 6, inside the linear layer body. This is not a reordering — it is a different domain. A curve applied to display-referred sRGB and the same curve applied in linear light are different operators.
- **Grain runs after the encode too**, and after curves.

So criterion 4 fails twice over: the order cannot be changed without editing the shader, *and* the order that is there is not one PhotoDesk could adopt as-is even if it were willing to renumber §5.

**Gate result: 4 criteria, 2 clean failures, 1 hollow pass, 1 pass. The fork does not proceed.**

---

## Subsystem classification

Fan-out columns count distinct `crate::` modules referenced (`uses`) and distinct modules referencing this one (`used-by`). "Headless" means callable with no webview and no front-end state.

| Subsystem | File(s) | LOC | uses / used-by | Headless | Coupling to `AllAdjustments` | Class |
|---|---|---|---|---|---|---|
| RAW decode | `raw_processing.rs` | 257 | 2 / 3 | yes | none | `ADAPT` |
| Demosaic | via `rawler` | — | — | yes | none | `ADAPT` |
| Colour management / ICC | **does not exist** | 0 | — | — | — | `REPLACE` |
| WGSL shader chain | `shaders/*.wgsl` | 2,480 | — | yes | **is** the layout | `REPLACE` |
| GPU resource / texture mgmt | `gpu_processing.rs` | 2,021 | 3 / 5 | yes (core), no (wrappers) | consumes it | `ADAPT` (reference) |
| Mask rasteriser | `mask_generation.rs` | 1,511 | 4 / 6 | yes | writes into it | `REPLACE` |
| Mask compositor | inside `shader.wgsl` | — | — | yes | **is** the layout | `REPLACE` |
| Edit-state representation | `image_processing.rs:1412–1624` | ~210 | 4 / 17 | yes | **is** it | `REPLACE` |
| Sidecar / persistence | `exif_processing.rs`, `file_management.rs` | 1,676 / 4,306 | 2 / 10, 17 / 11 | partly | serialises it untyped | `REPLACE` |
| Preset system | `preset_converter.rs` | 349 | 1 / 1 | yes | XMP → their JSON | `AVOID` |
| History / undo | **does not exist in Rust** | 0 | — | — | — | `REPLACE` |
| Thumbnail / proxy generation | `file_management.rs`, `cache_utils.rs` | ~600 / 312 | — / 6 | no | keyed on it | `REPLACE` |
| Export and encoding | `export_processing.rs` | 1,701 | 12 / 1 | no | consumes it | `REPLACE` |
| AI / segmentation | `ai_processing.rs`, `ai_commands.rs` | 1,759 / 430 | 1 / 6, — | partly | independent | `ADAPT` (reference) |
| AI remote bridge | `ai_connector.rs` | 224 | 0 / 2 | yes | independent | `ADAPT` (reference) |
| IPC command surface | 116 `#[tauri::command]` across 24 files | — | — | n/a | — | `AVOID` |
| File IO | `file_management.rs` | 4,306 | 17 / 11 | no | — | `AVOID` |
| File watching | **does not exist** | 0 | — | — | — | n/a |
| Front end | `src/**` (TS/TSX/CSS) | 53,160 | — | n/a | — | `AVOID` |

**Totals:** 35,601 lines of Rust, 53,160 of front end, 88,761 together.

Notes on the entries that need one:

- **`AVOID` on the preset system** — it converts Lightroom XMP into RapidRAW's parameter names. §6.1 defines a preset as a PhotoDesk document minus `source`/`geometry`/`output`. Nothing transfers. Noted so it isn't reintroduced as "we already have XMP import".
- **`AVOID` on the IPC surface** — 116 commands, 43 of them in `file_management.rs`, shaped around RapidRAW's front end. §2.1 predicted a rind rather than a marbling and was right; the rind is simply not ours.
- **`ADAPT` (reference)** on the three GPU/AI rows means: read it, learn from it, write our own. It does *not* mean vendoring. Given the verdict there is no vendored code at all, and "reference" is the honest label for the value that remains.
- **`rawler` is a fork**, `CyberTimon/RapidRAW-DngLab`, not upstream `dnglab/rawler`. Since we are not forking RapidRAW, this constraint lifts: PhotoDesk should evaluate **upstream `rawler`** and inherit an upstream rather than somebody else's fork. §2.1 flagged tracking-a-fork as a cost; declining the fork refunds it.

---

## The four findings that decide it

Criterion 4 is sufficient on its own. These three are recorded because each would have surfaced in month two or three, and each is a fact about what the fork would and would not have delivered.

### 1. There is no colour management. None.

A case-insensitive search of all 35,601 lines of Rust for `icc`, `lcms`, `colorspace`, `color_profile`, `display.p3`, `adobe.rgb`, `prophoto`, `rec2020`, `chromatic adaptation`, `white point` and `primaries` returns **one** substantive cluster: `image_processing.rs:1784–1862`, which builds Rec.2020 primaries matrices as internal machinery for the AgX tonemapper's gamut compression. That is a tone-mapping implementation detail, not colour management.

There is no ICC parser. No embedded-profile extraction. No P3 path. The shader does `srgb_to_linear` on input (`shader.wgsl:1640`) and `linear_to_srgb` on output (`:1840`), unconditionally. Export writes the EXIF `ColorSpace` tag as the hard-coded constant `1` — sRGB (`exif_processing.rs:1516`).

**RapidRAW is an sRGB-only pipeline that assumes every input is sRGB.** A Display P3 iPhone photograph opened in it is silently misinterpreted, which is precisely the failure Spike B's third test (`Untagged input assumed sRGB`, max ΔE < 1.0) exists to catch — except here it applies to *tagged* input too.

§4 is not a refinement of this. §4 would be built from nothing.

### 2. It cannot open the format this application exists to edit.

`formats.rs:73` lists `NON_RAW_EXTENSIONS`. It contains jpg, jpeg, png, gif, bmp, tiff, tif, webp, jxl, exr, hdr, tga, ico, dds, qoi, ff, and the Netpbm family.

It does not contain `heic`. Or `heif`. Or `avif`. There is no `libheif` in `Cargo.toml`, and a search of the whole tree for `heif|heic|libheif` returns nothing.

§1 names the native subject as "an iPhone HEIF". §14 makes v0.1 literally "Open iPhone HEIF → …". The fork base cannot open it, and adding HEIF means adding a decoder, which means adding the ICC handling from finding 1, which means the first two things v0.1 needs are both greenfield inside the fork.

### 3. On Linux, the preview is a JPEG round-trip — the opposite of §7.2's candidate.

`lib.rs:359–362`:

```rust
#[cfg(any(target_os = "linux", target_os = "android"))]
let use_wgpu_renderer = false;
```

And `gpu_processing.rs:206`: on Linux and Android, `surface_opt` is hard-`None`. The only `@fragment` shader in the project, `display.wgsl`, is `include_str!`'d at `gpu_processing.rs:305`, inside the `#[cfg(not(any(target_os = "android", target_os = "linux")))]` block. **On Linux it is never compiled. RapidRAW's Linux shader chain is 100% compute.**

What happens instead, per frame, during a slider drag: GPU compute writes `rgba8unorm` → CPU readback → **mozjpeg encode at quality 65, 75 or 85** depending on the `live_preview_quality` setting (`lib.rs:355`, `:365–369`, `:571`) → binary blob over IPC → webview.

RapidRAW independently hit the same wall §7.2 names (tauri-apps/tauri#9220) and resolved it by going *all the way to the other end*: maximum per-frame IPC, lossy 8-bit transport. §7.2's candidate was "zero per-frame IPC". The fork therefore contributes **nothing** to the preview path. Spike C would have had to be run in full regardless of Spike A's outcome — which is worth knowing, because it means the two spikes were less coupled than §2.3 assumed.

### 4. 35,601 lines of Rust, zero tests.

`grep -rn '#\[test\]\|#\[cfg(test)\]'` across `src-tauri/src` returns **0**. There is no `tests/` directory. There is no golden-image corpus, no colour harness, no fixture set.

§12 opens: "The regression suite is not optional infrastructure — for a non-destructive editor it *is* the correctness argument." A fork would have inherited 35,601 lines of code with no correctness argument attached, and every test in §12 would have had to be written from the outside against behaviour nobody had pinned down. That is not a reason to refuse a fork by itself. It is a large, quiet line item that belonged in the estimate and was not in it.

---

## Smaller observations, kept because they are cheap to lose

- **Output is `texture_storage_2d<rgba8unorm, write>`** (`shader.wgsl:198`) — 8-bit, clamped to [0,1] with a 1/255 dither at `:1907`. §4's f16 working space is not representable at the output stage without changing the binding, i.e. editing the shader.
- **`MAX_MASKS = 32`** (`image_processing.rs:1613`) is baked into both the Rust struct and the WGSL array declaration. §7.3 bounds PhotoDesk at 6 layers for the frame budget; the mismatch is not a problem, but the *fixedness* is — the count is part of the buffer layout, so it is a shader edit either way.
- **Cache keys use `std::collections::hash_map::DefaultHasher`** over `serde_json::Value::to_string()` (`cache_utils.rs:29`, `:66`). `DefaultHasher` carries no stability guarantee across Rust releases, so a disk cache keyed on it can go stale or collide across a toolchain upgrade. §9.3's scheme is materially more rigorous, and this is a clean `REPLACE` with a written reason.
- **The `.rrdata` sidecar has a `version: u32` field that is never read.** `ImageMetadata` is defined at `image_processing.rs:53`; a search for `.version ==`, `.version >`, `.version <` across the tree returns nothing. There is no migration path. Worse, `load_sidecar` (`exif_processing.rs:224`) parses with `.unwrap_or_default()` — **a malformed or future-schema sidecar silently becomes an empty document and every edit in it is discarded.** §6.3's table says reject-with-a-message in exactly this case, on the argument that a partially-understood edit is worse than a refused one. This is the strongest possible confirmation of that rule, found in the wild.
- **`adjustments` inside the sidecar is an untyped `serde_json::Value`.** §6.1 requires a fixed schema per `op` + `op_version` with unknown keys as a validation error. Here unknown keys are a silent no-op — the specific outcome §6.1 forbids by name.
- **Masks rasterise on the CPU**, not the GPU: `generate_radial_bitmap`, `generate_linear_bitmap`, `generate_brush_bitmap` all return `image::GrayImage` (`mask_generation.rs:539`, `:583`, `:637`), which is then uploaded as a `texture_2d_array`. §8 specifies GPU rasterisation for both parametric and brush components, the latter explicitly "so proxy and export agree". Different architecture, different cost, different agreement properties.
- **Segmentation is SAM ViT-B**, not SAM 2.1 Hiera Tiny — `sam_vit_b_01ec64_encoder.onnx` / `_decoder.onnx`, fetched at runtime from a HuggingFace repo (`ai_processing.rs:22–25`), plus `u2net`, `skyseg-u2net` and a CLIP model. The model differs from §9.2's initial choice, but **the encoder/decoder split §9.2 designs `Segmenter` around is exactly what they do** — independent confirmation that `prepare`/`segment` is the right two-phase interface.
- **`ai_connector.rs` is not a ComfyUI client.** The single occurrence of "comfy" in the entire Rust tree is a serde field alias (`app_settings.rs:426`). What exists is a bespoke HTTP client for RapidRAW's own service — `POST /upload_source`, `POST /inpaint`, `GET /health` (`ai_connector.rs:95`, `:159`, `:132`). It maps well onto §9.1's `RemoteHttp` shape, and the presence of a dedicated `/health` endpoint is quiet support for `Provider::health()` being a separate cheap call.
- **`GlobalAdjustments` has a field named `centré`** (`image_processing.rs:1495`), with the acute accent, mapping positionally to `centre` in the WGSL. Harmless under `#[repr(C)]`, and it would serialise under that name. Noted only as a reminder that the struct is a GPU buffer layout first and an API second.
- **`wgpu` is pinned to 29.0** with the comment "Downgraded to prevent P3 color shifts on Apple devices". Their colour path has known version sensitivity in a codebase with no colour management, which suggests the sensitivity is in the surface/swapchain rather than in a transform. Feeds Spike B as a caution, not as a finding.

---

## Register consequences

| Register item | Was | Now | Basis |
|---|---|---|---|
| **Fork RapidRAW** | PROVISIONAL → Spike A | **FROZEN: do not fork.** Build against `rawler` + `libheif` + our own shaders. | This document. Criterion 4 fails on the shader chain; criterion 3 fails 1-of-3; findings 1 and 2 remove colour management and the native input format. |
| **Front-end framework** | PROVISIONAL → Spike A | **PROVISIONAL → Spike C.** | Spike A's exit is consumed without resolving it: §16 #8 says "no fork leaves it open", and there is no fork. §0 forbids a provisional item without a named exit, so it is re-pointed at the next spike that actually puts pixels in the webview. |
| **Preview renderer path** | PROVISIONAL → Spike C | PROVISIONAL → Spike C, **reframed again** | See below. §2.3's "hand-written GLSL is the expected outcome" rested on RapidRAW's chain being compute. It is no longer our chain. |
| `engine/` in §13 | "Absent if the gate failed" | **Absent.** | Gate failed. |
| `shaders/photodesk/` in §13 | "Absent if the chain classified ADAPT" | **Exists.** It is the only shader source. | The §13 ambiguity is resolved by the gate failing: there is exactly one shader source and it is ours. |

Nothing else in the register moves. Spikes B and C are untouched as questions, though C changes shape.

### What this changes

**§2.3's expected outcome is now open again, and that is the single largest piece of good news in this document.**

§2.3 reasoned: RapidRAW's chain is compute end to end; GLSL ES 3.0 has no compute shaders, no storage textures and no storage buffers; therefore `naga` has nothing to lower onto; therefore hand-write ~15 fragment shaders. Every step is correct **about RapidRAW's chain**. None of it is binding on a chain we author ourselves.

If PhotoDesk writes its WGSL fragment-first from day one — `@fragment` entry points, uniform buffers rather than storage buffers, sampled textures rather than storage textures, rendering to a framebuffer attachment rather than `textureStore` — then the constructs that had no GLSL ES 3.0 target are simply never used, and §7.2's "author once in WGSL, transpile with `naga`" is a live candidate again rather than a foreclosed one.

Two things make this fit rather than merely possible:

- **§5's per-layer loop is what makes the uniform buffer work.** RapidRAW needs 32 mask-adjustment blocks resident at once because it blends parameters in a single pass — which is why it needs a storage buffer. PhotoDesk runs the loop body once per layer, so a pass needs exactly one layer's parameters: tone curves dominate at 4 × 16 points, and the whole block lands comfortably under 1 KB against GLES3's guaranteed 16 KB uniform-buffer minimum. The model chosen in §5 for correctness reasons happens to be the one that fits the transport constraint.
- **§7.3 already requires stages 2–9 fused into a single pass.** A fused per-pixel pass *is* a fragment shader. The architecture was already pointing this way.

**This is a hypothesis, not a finding, and Spike C must now test it rather than assume the fallback.** `naga`'s GLSL backend targeting `300 es` has to be run against a real fragment shader of ours before anything is claimed. Spike C's success criteria are unchanged otherwise, including `EXT_color_buffer_float` for an RGBA16F colour-renderable target with linear filtering — that clause matters more now, not less, since finding 3 shows the fork would not have supplied a preview path either way.

---

## Recommendation on the gate itself

Criterion 2 passed as written while the invariant behind it failed. That is the same defect the 2026-09-05 review found in criterion 2's earlier wording, and the fix that produced criterion 4 did not generalise.

The gate is spent — there is no second fork candidate queued — so this is a note for the next time a gate is written rather than a change to make now. Stated so it is on the record: **a criterion phrased over a mechanism ("can X be swapped behind an adapter") will be satisfied by any sufficiently trivial adapter. Phrase it over the invariant instead** — "can a two-layer PhotoDesk document with distinct masks and opacities be rendered by this engine, unchanged?" — which criterion 2 would have failed immediately, and for the right reason.

---

## Licensing

RapidRAW is AGPL-3.0 (`LICENSE`, GNU Affero General Public License v3).

§16 sequenced the licensing review to gate Spike A's conclusion, on the argument that the fork decision commits months of work and this repository is public. **The engineering gate failed first, so the licensing review is no longer on the critical path for that decision** — no AGPL code is being vendored, adapted, or redistributed, because there is no fork.

Two things remain true and are recorded rather than resolved:

- This document quotes RapidRAW identifiers, line numbers and short fragments for the purpose of the audit. No RapidRAW source has been copied into this repository, and none should be.
- "Architectural reference" means reading their code and then writing ours. That is a distinct question from vendoring, it is one people take seriously, and it is the user's to raise in the licensing review if a review still happens. Nothing here is legal advice.

**The specification-only constraint on this repository can now lift.** It existed because the fork decision was live and the repository is public; the fork decision is closed, and the code that follows is original.

---

## What to build instead

Not a plan — §14 owns the plan. This is the shape the fallback takes, so the next session starts from something.

| Need | Source | Status on this machine |
|---|---|---|
| HEIF/HEIC decode | `libheif` via a Rust binding | runtime present, **`libheif-devel` absent** |
| ICC parse + transform | `lcms2` | **`lcms2-devel` present** |
| RAW decode + demosaic | **upstream `rawler`** (`dnglab/rawler`), not the RapidRAW fork | crate, no system dep |
| JPEG/PNG/WebP encode | `image`, `mozjpeg` | crate |
| Shaders | ours, fragment-first WGSL | greenfield |
| Preview transport | Spike C decides | greenfield |

Spike B is unblocked and unchanged: it needs `libheif-devel` and `lcms2`, it does not need Spike C, and it is now the only thing standing between this project and a frozen colour architecture. §4 has to be built from nothing rather than adapted, which raises Spike B's importance without changing its design — the harness was already specified to prove the working space rather than assume it, and it now proves a working space with no incumbent behind it.

Two spikes remain. Neither was resolved by this one, and one of them just got its expected outcome reopened in a favourable direction.
