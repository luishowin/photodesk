# Spike C — preview renderer

**Date:** 2026-09-05
**Subject:** §7.2's candidate — author once in WGSL, transpile to GLSL ES 3.0 for the webview
**Harness:** `tests/renderer/`, 6 Rust tests + a WebKitGTK probe
**Deliverable for:** `ARCHITECTURE.md` §2.3

---

## Verdict

**Freeze the preview path: WebGL2 in the webview, WGSL authored fragment-first, transpiled to GLSL ES 3.00 by `naga` at build time. §7.2's original candidate, not its fallback.**

Four things had to hold. All four do:

| Question | Answer |
|---|---|
| Does a realistic WGSL pass lower to GLSL ES 3.00? | **Yes.** 9,292 bytes of ES 3.00 from a 256-line fused pass. |
| Does WebKitGTK compile it? | **Yes.** Compiled and linked, no GL error. |
| Is the working space representable there? | **Yes.** `EXT_color_buffer_float` present, RGBA16F colour attachment `COMPLETE`, linear filtering measured at 0.5. |
| Is it at budget? | **Yes.** Six layers at 2 MP: **5.92 ms median, 6.08 ms p95** against 16 ms. |

And the question §2.3 did not ask, which is the one §0 actually cares about:

| | |
|---|---|
| Do the two paths produce the same pixels? | **Yes.** max \|diff\| **0.0088**, mean **0.0002**, 3 of 196,608 channel samples over 1/255. |

**Phase 0 is complete.** All three spikes have run; §14's v0.1 is unblocked.

---

## The finding that reopened this

Spike A killed §7.2's candidate on the evidence that the chain to be transpiled was compute end to end. That chain was RapidRAW's, and there is no fork — so §2.3 was rewritten around a hypothesis: author fragment-first, never use the constructs GLSL ES 3.0 lacks, and the candidate comes back.

The hypothesis is now tested from both sides.

**Positive:** a `@fragment` entry point with a `var<uniform>` block, sampled textures and a `@location(0)` return lowers cleanly.

**Negative control, which matters more:** the same harness feeds `naga` a compute shader of exactly RapidRAW's shape — `@compute`, `var<storage, read>`, `texture_storage_2d<rgba8unorm, write>`, `textureStore`. It refuses, and names its reasons:

```
MissingFeatures(Features(BUFFER_STORAGE | COMPUTE_SHADER | IMAGE_LOAD_STORE))
```

Those are precisely the three constructs §2.3 identified as having no GLSL ES 3.0 target, returned by the tool rather than reasoned about. The fragment result is therefore a *consequence* of authoring fragment-first, not a coincidence — which is what the control was there to establish.

---

## What was actually transpiled

Not a toy. §2.3 says the outcome "decides how every shader in the project is written, and that is not a decision to discover twenty shaders in", so the test shader is §5 stages 2–9 fused into a single pass — the arrangement §7.3 requires — and it deliberately contains the constructs most likely to break a GLSL ES 300 backend rather than the ones most likely to survive:

- a large uniform block with **four 8×vec4 arrays** in it (tone curves, packed two control points per vec4)
- **dynamic indexing** into those arrays
- a **data-dependent loop bound** (`for i in 1..count`)
- a **`switch`** with a default arm
- `%` on floats and signed ints, `smoothstep`, `exp2`, `pow`, `atan`-free HSV round trip
- a full-screen triangle derived from `@builtin(vertex_index)`, so there is no vertex buffer

All of it survived. The emitted GLSL declares `layout(std140) uniform Adjustments_block_0Fragment`, keeps the arrays as arrays, and preserves `gl_VertexID`.

---

## Results in detail

### §2.3's capability clause

This is the clause the whole spike was sequenced around: §2.3 warns that Spike B and Spike C can each be green and jointly wrong if the working space turns out not to be renderable in the webview. Spike B froze f16 without touching a GPU. So this was checked first.

```
EXT_color_buffer_float ................ true
RGBA16F colour attachment ............. FRAMEBUFFER_COMPLETE
RGBA16F linear filtering .............. 0.5 at the midpoint  -> LINEAR
```

The filtering result is **measured, not inferred**. `OES_texture_half_float_linear` reports `false` here, and reading that as a failure would have been wrong: the extension does not exist in WebGL2 because RGBA16F filtering is core there, so the string proves nothing in either direction. The probe therefore samples a 2×1 texture of 0 and 1 exactly between the texels — 0.5 means LINEAR, 0 or 1 means NEAREST. It returns 0.5.

**Spike B's freeze is safe.** The risk §2.3 named is retired.

### §7.3's budget

2 MP proxy (1920×1080), RGBA16F throughout, ping-ponged between two render targets so a multi-layer stack is real passes rather than one pass measured repeatedly.

| Layers | median | p95 | verdict |
|---|---|---|---|
| 1 | 1.00 ms | 1.33 ms | within |
| 2 | 2.75 ms | 3.58 ms | within |
| 4 | 4.17 ms | 6.33 ms | within |
| **6** | **5.92 ms** | **6.08 ms** | **within — §7.3's bound** |
| 8 | 7.83 ms | 8.08 ms | within |

Roughly **1 ms per layer at 2 MP**, scaling linearly. At §7.3's six-layer bound the frame costs about **37% of the 16 ms budget**, which leaves room for the spatial stages (10–12) that this pass does not yet include and for the upload of a freshly decoded image.

§7.3 also allows frame rate to fall beyond 6 layers and requires the UI to say so. On this hardware it does not need to until far past that: 8 layers is still half the budget.

**A caveat on the numbers.** WebKit clamps `performance.now()` to 1 ms against timing attacks. The first run of this probe reported a table of exact whole milliseconds, which was an artefact of the clock and not a property of the renderer. Each sample now times a batch of 12 frames and divides, putting the measured interval well above the clamp — that is why the figures have meaningful decimals.

### §0's invariant — the two paths agree

Compiling is not correctness. §0 freezes "one shader source, preview and export" on the argument that separate paths guarantee undiscoverable WYSIWYG drift, and a transpiled shader that compiles but computes something slightly different is that drift wearing a build step as a disguise.

So `tests/renderer/tests/agreement.rs` renders the **same WGSL** through wgpu on the native path — Vulkan, RADV RENOIR, headless — and writes the result where the browser can fetch it. The probe renders the transpiled GLSL over the same source image with the same parameters and compares.

```
196,608 channel samples (256 x 256, RGB)
max |diff|   0.008789      (~2.2 / 255)
mean         0.0001944     (~0.05 / 255)
over 1/255   3 samples
orientation  direct        (flipped scores 1.122)
```

Both sides store RGBA16F, so the comparison is about shader maths rather than about one path carrying more precision. Both sample NEAREST, so filtering differences cannot be mistaken for arithmetic ones. The parameter block is **not shared as bytes**: the native side writes a `#[repr(C)]` struct, the browser places named fields at std140 offsets it queries from the linked program. Agreement therefore means the two layouts genuinely match, rather than that they read the same buffer.

The flipped score of 1.122 is the control: it shows the comparison is sensitive to a one-row misalignment, so "direct, 0.0088" is a real match and not a metric that would have reported success on anything.

The residual 0.0088 is f16 rounding plus differing implementations of the transcendentals (`pow`, `exp2`, `sin`) between the two compilation paths — expected, and roughly two 8-bit codes at the very worst pixel out of nearly two hundred thousand.

### §2.3's uniform-buffer claim

§2.3 argues that §5's per-layer loop is what makes a uniform buffer sufficient where RapidRAW's 32-slot mask array needed a storage buffer. Measured:

```
Adjustments_block_0Fragment = 784 bytes
MAX_UNIFORM_BLOCK_SIZE      = 65,536 bytes   (GLES3 guarantees 16,384)
```

**784 bytes of a guaranteed 16 KB.** The claim holds with two orders of magnitude to spare, and tone curves — 4 × 16 control points — are the bulk of it.

---

## Two corrections during the spike

Both produced confident-looking numbers that were measuring something other than what they claimed.

**A stray debug line was raising `GL_INVALID_VALUE` on every run.** `getUniformIndices` for a uniform that does not exist returns `INVALID_INDEX`, and feeding that to `getActiveUniform` raises `0x501`. The first probe reported "within budget" for every layer count with an error outstanding — which could equally have meant draws were being dropped. Removed; the run now ends with `gl error: none`, and the budget table is trustworthy because of that and not despite it.

**The first agreement run reported max 1.13 and 196,027 of 196,608 samples out of tolerance** — an apparently catastrophic disagreement. It was not a shader problem. `naga` names a block member three different ways depending on its type:

```
scalar   _group_0_binding_0_fs.exposure
array    _group_0_binding_0_fs.luma_curve[0]
struct   _group_0_binding_0_fs.grade_shadows.offset
```

The lookup matched only the first, so the tone curves, the HSL bands and all four grading wheels silently resolved to nothing and the browser rendered with zeros where the native reference had real values. The two sides were being fed different inputs, and the harness dutifully reported the difference as though it were drift.

The lookup now handles all three forms and queries `UNIFORM_ARRAY_STRIDE` rather than assuming 16 bytes — **and a missing field is now a hard failure that refuses to produce a number at all.** That second change is the important one: the original failure mode was not the wrong answer, it was a wrong answer that looked like a finding.

---

## What is not proven

- **The engine is right; the binding is not identical.** This ran in **webkitgtk-6.0** via Epiphany 50. Tauri v2 on Linux uses **webkit2gtk-4.1**. Both are installed here, both are WebKit 2.52.5, and they share the WebGL implementation — but this has not been confirmed inside Tauri's own webview, and that confirmation belongs to the first v0.1 build.
- **The renderer string is masked.** WebKit reports `Apple GPU` for fingerprinting resistance. The native side identifies the real device as `AMD Radeon Graphics (RADV RENOIR)`; the browser side cannot be asked to confirm it is using the same one.
- **Synthetic input.** A procedural 2 MP image, not a decoded photograph. Fine for a per-pixel cost measurement — the shader does not care what the pixels are — but the decode and upload path is untested and is part of §7.3's separate 800 ms open-to-first-render budget.
- **Stages 10–12 are absent.** This pass is stages 2–9, which is what §7.3 requires to be fused. The spatial stages need their own passes and their own measurement; the ~10 ms of headroom at six layers is where they have to fit.
- **Every layer ran the same shader.** A real stack varies parameters per layer, which this does exercise, but not shader permutations — and if stages get specialised out into variants, pipeline-switching cost is a new question.
- **The interactive harness has not been watched by a human.** `web/preview.html` exists and runs the transpiled shader with real sliders at 2 MP; the automated probe covers the same code path, but §2.3's success criterion is phrased as a slider moving an image and that is worth seeing once.

---

## Register consequences

| Item | Was | Now |
|---|---|---|
| **Preview renderer path** | PROVISIONAL → Spike C | **FROZEN.** WebGL2 in the webview; WGSL authored fragment-first; `naga` → GLSL ES 3.00 at build time; wgpu natively for export. |
| **One shader source, preview and export** | FROZEN, untested | **FROZEN and now measured** at max 0.0088 across 196,608 samples. §12.2 inherits this comparison. |
| **Working space = linear P3 f16** | FROZEN by Spike B | **Unchanged, and no longer at risk.** RGBA16F is colour-renderable and filterable in the target webview. |
| **Front-end framework** | PROVISIONAL → Spike C | **Spike C removes the constraint rather than making the choice** — see below. |

### On the front-end framework

Spike C was asked to resolve this because it is the first thing that puts pixels in a webview. It has an answer, and the answer is that **the renderer does not care**.

The canvas path is a WebGL2 context, a uniform buffer, three draw calls and a `requestAnimationFrame` loop. It touches no framework API, needs no reconciliation, and would be identical under React, Svelte, Solid or none. `web/preview.html` is plain ES modules with no build step at all, and it runs the full stack at 2 MP.

So the constraint Spike C was meant to discover does not exist, and what remains is a UI-ergonomics decision about §10's panels and §11's interaction surface — not a rendering one. That is a decision for the author rather than for a spike, and it is the last thing standing between here and v0.1 scaffolding.

---

## Running it

```
cargo test -p photodesk-renderer-spike           # transpile + native reference
python3 tests/renderer/web/run-probe.py          # the WebKitGTK probe, reports everything
python3 tests/renderer/web/run-probe.py --keep-open   # leave the browser up
```

The probe opens a short-lived Epiphany window, waits for the page to POST its report, and closes it. `web/preview.html` is the interactive harness — serve the `web/` directory and open it to drag sliders against the transpiled shader.
