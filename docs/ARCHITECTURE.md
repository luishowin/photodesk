# PhotoDesk — Architecture Specification

**Version:** 0.4 (pre-code)
**Author:** Luis Howin
**Platform:** Fedora Workstation / GNOME
**Status:** Master spec for the coding agent.

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
| **Fork RapidRAW** | PROVISIONAL | → Spike A (§2.1) |
| **Working space = linear Display P3 f16** | PROVISIONAL | → Spike B (§2.2) |
| **Preview renderer path** | PROVISIONAL | → Spike C (§2.3). Naga transpilation is now the unlikely branch — see the compute-chain finding in §2.1. |
| **v1 pipeline stage ordering** | PROVISIONAL | → golden-image validation (§12.1) |
| Document is stack-shaped on disk | PROVISIONAL | Until the stack becomes lossy for the graph (§6.2) |
| Which AI providers ship | PROVISIONAL | Interface frozen, implementations swap freely |
| **v1 discards iPhone HDR gain maps** | PROVISIONAL | → an HDR display, or the first wanted gain-mapped export (§4) |
| **Front-end framework** | PROVISIONAL | → Spike A (§2.1). A fork inherits React + Vite; no fork leaves it open. |

---

## 1. Philosophy

PhotoDesk is a **display-referred photo editor**. Its native subject is a finished image — an iPhone HEIF, a JPEG, a screenshot — that already has a rendering intent baked in by the device that made it.

It is **not** a RAW laboratory. It doesn't reconstruct scene radiance, doesn't ask you to pick a demosaic algorithm or a view transform. RAW is an *input*, not a worldview.

**The test for any feature:** does it shorten the path between opening a photo and being happy with it?

**Non-goals:** cataloguing, cloud sync, face recognition, tethering, print workflows, compositing, scene-referred view transforms, camera profile management, demosaic pickers, soft-proofing UI, a node editor UI, plugins for other people.

---

## 2. Phase 0 — three spikes before any product code

Nothing in §4 onward is safe to build until these three questions are answered. Budget roughly **two weeks with no visible product**. That is not wasted time; it's the price of not discovering any of this in month four.

### 2.1 Spike A — RapidRAW fork audit

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

### 2.2 Spike B — colour validation harness

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

### 2.3 Spike C — preview renderer

Establish whether the same WGSL can drive both paths on Fedora (§7.2). Success is a slider moving an image at proxy resolution inside the webview, at budget (§7.3), **and the working space actually representable** — RGBA16F as a colour-renderable target with linear filtering, which on WebGL2 means `EXT_color_buffer_float` on this machine's WebKitGTK. That second clause is not padding: without it Spike B can freeze f16 against a preview path that turns out unable to render to it, and the two spikes would each be individually green and jointly wrong.

**The audit has already made the first question much less open.** Per §2.1, RapidRAW's chain is compute end to end — `@compute` entry points, a `texture_storage_2d` output, a storage buffer of adjustments. GLSL ES 3.0 has no compute shaders, no storage textures and no storage buffers, so this is not `naga` having gaps; there is no target construct to lower those onto. Plan on §7.2's fallback as the expected outcome rather than a contingency: **the preview chain is hand-written as WebGL2 fragment shaders.** One shader source is then preserved by making the shared artefact the *shader body* — the per-pixel maths in an included file, emitted for both targets — rather than the dispatch mechanism around it. If that sharing proves unworkable, the thing under threat is §0's one-shader-source invariant itself, and it gets escalated to a register change rather than quietly patched.

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

---

## 4. Colour architecture — *provisional, pending Spike B*

**Candidate working space: linear Display P3, f16.**

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

**HDR gain maps — v1 ignores them, and says so out loud.** A modern iPhone HEIC ships an SDR base image plus an ISO gain map that Photos.app applies on an HDR display. PhotoDesk v1 decodes the SDR base and discards the gain map. The reason it has to be *written down* rather than merely implemented: §1's thesis is to take the manufacturer's rendering as the starting point, and on an HDR display the manufacturer's rendering *is* the gain-mapped one — so an unstated drop means the app opens a photo looking flatter than the Photos.app the user just came from, and they conclude the colour pipeline is broken. It isn't; it's this decision. It is the right decision for v1 anyway, because the target display (§4) is a 60–70% sRGB laptop IPS that cannot show the difference. **Exit condition:** an HDR-capable display, or the first time a gain-mapped export is actually wanted. `image-hdr` is already in RapidRAW's dependency tree if that day comes.

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
├── IMG_4821.photodesk.json
└── .photodesk/
    ├── proxy/…      ← regenerable
    ├── masks/…      ← regenerable
    └── embed/…      ← regenerable
```

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

### 7.2 Where the shaders run — *provisional, pending Spike C*

Tauri on Linux uses a GTK/WebKit webview, and the native-wgpu-behind-the-webview approach doesn't work there — the two contend for the same surface and flicker (tauri-apps/tauri#9220).

**Candidate:** preview in the webview via WebGL2; export in Rust via wgpu; author once in WGSL and transpile to GLSL ES 3.0 with `naga` at build time. Zero per-frame IPC, and the interactive path lands in a technology already written fluently here.

**Risk, now measured rather than suspected (§2.1):** the chain to be transpiled is compute-based, and `naga`'s GLSL backend cannot lower compute shaders, storage textures or storage buffers to GLSL ES 3.0 because those constructs do not exist there. **Fallback:** hand-write the preview chain in GLSL. Roughly fifteen fragment shaders of well-understood per-pixel math — tedious, not hard. WebGPU in WebKitGTK is the eventual clean answer but not yet dependable.

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

### 12.1 Golden images

A committed corpus of ~12 sources × ~8 documents, rendered and compared to blessed reference PNGs by ΔE2000 (max and mean thresholds per case).

That is ~96 reference images, so fix the storage question **before the first `--bless`, not after**: references render at proxy resolution (~2 MP), 16-bit PNG, and both the source corpus and the references live in **git-lfs**. Retrofitting lfs onto a repository that already has a hundred binaries in its history is a rewrite of that history.

**Blessing workflow — the part that's usually skipped and then poisons the suite:** intentional pipeline changes will fail these tests by design. `cargo test --bless` regenerates references, but the diff must be reviewed **visually** in a side-by-side report, and the blessing is a separate commit citing the change that caused it. Blind blessing turns a regression suite into a rubber stamp within a month.

### 12.2 Proxy / full-res agreement

Render the same document at proxy and at full-res-downsampled-to-proxy. Assert they match within threshold.

This is the test that enforces the one-shader-source invariant. Without it the invariant is a comment.

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

### 12.4 Others

- **Migration:** every version step has a fixture pair, plus rejection tests for newer schemas and unknown ops.
- **Cache invalidation:** mutate each key component in turn, assert the cache misses.
- **AI-absent:** delete the provider binaries, run the full corpus, assert open/edit/export all succeed.
- **Performance:** assert the §7.3 budgets on the reference machine. Failures are warnings, not build breaks — but they're recorded per commit so drift is visible.

---

## 13. Repository

```
photodesk/
├── src/                        ← front end
│   ├── document/               ← model, validation, migration, history, presets
│   ├── graph/                  ← DAG compile, dirty tracking (no UI)
│   ├── panels/                 ← Crop Light Color Detail Effects Masks AI
│   ├── canvas/                 ← viewport, preview renderer, before/after
│   └── design/                 ← tokens, type scale
├── src-tauri/src/
│   ├── engine/                 ← vendored upstream (per Spike A). Read-only. Absent if the gate failed.
│   ├── photodesk/              ← document → engine bridge, IO, cache, export
│   └── ai/                     ← provider registry, routing
├── shaders/photodesk/          ← WGSL source of truth. Absent if the chain classified ADAPT
├── providers/                  ← LocalCpu, RemoteHttp, ComfyUI, LocalGpu
├── tests/
│   ├── golden/                 ← corpus + blessed references
│   ├── color/                  ← Spike B harness, kept as a permanent suite
│   └── fixtures/migrations/
├── docs/{ARCHITECTURE,FORK-AUDIT,PIPELINE,DOCUMENT,DECISIONS}.md
└── packaging/rpm/
```

**Exactly one of `engine/`'s shader chain and `shaders/photodesk/` exists.** Which one is Spike A's output (§2.1). Two live shader sources is the WYSIWYG drift §0 freezes against, wearing a directory layout as a disguise.

`DECISIONS.md` is an append-only log: each entry records what moved from PROVISIONAL to FROZEN, the spike that resolved it, and the date. The §0 register is the current state; this is the history.

**Packaging is one RPM.** No AppImage, Flatpak or DEB. A build matrix for an audience of one is the mad lab wearing a different hat.

---

## 14. Roadmap

| Version | Scope | Estimate |
|---|---|---|
| **0.0** | Spikes A, B, C. `FORK-AUDIT.md`, colour harness green, renderer path chosen. **No product.** | 2 weeks |
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
| 1 | Fork or build from scratch | Spike A |
| 2 | Working space and precision | Spike B |
| 3 | Preview renderer path | Spike C |
| 4 | Pipeline v1 ordering | Golden-image validation |
| 5 | Pre-1.0 vs post-1.0 reorder policy | Before v0.2 (§5) |
| 6 | Remote AI endpoint: self-hosted ComfyUI or gateway | Before v0.5 |
| 7 | Icon — `PD` monogram or geometric mark, monochrome, no aperture | Whenever; the 16×16 render is the only test |
| 8 | Front-end framework | Spike A — a fork inherits React + Vite + i18next; no fork leaves it open |
| 9 | HDR gain map handling beyond v1's discard | An HDR display, or the first wanted gain-mapped export (§4) |
| 10 | Layer count at which frame rate is allowed to fall | Measured against the §7.3 bound of 6 |

**Licensing:** RapidRAW is AGPL-3.0. A licensing review is required before any redistribution, publication or portfolio use.

**Sequence that review to gate Spike A's conclusion, not shipping.** The fork decision commits months of work; discovering afterwards that the licence forecloses the intended use means discovering it at the most expensive available moment, which is the same failure mode §2 exists to prevent everywhere else. The repository is public from v0.0, which makes the question live now rather than later — and is a reason to keep the repository spec-only until the gate is decided.

The review itself is out of scope for this document and is not an engineering decision. Nothing here should be read as legal advice.
