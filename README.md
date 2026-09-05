# PhotoDesk

A display-referred photo editor for Fedora / GNOME.

Its native subject is a finished image — an iPhone HEIF, a JPEG, a screenshot — that already has a rendering intent baked in by the device that made it. It is not a RAW laboratory: it doesn't reconstruct scene radiance, doesn't ask you to pick a demosaic algorithm or a view transform. RAW is an *input*, not a worldview.

The test for any feature: **does it shorten the path between opening a photo and being happy with it?**

**Status page:** https://luishowin.github.io/photodesk/

---

## Status — v0.0, pre-code

There is no application yet, and that is on purpose. The architecture spec commits to three spikes before any product code, on the argument that discovering their answers in month four is far more expensive than spending three weeks on them now.

| Phase | State |
|---|---|
| Spec | v0.4 — [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| Spike A — RapidRAW fork audit | not started |
| Spike B — colour validation harness | not started |
| Spike C — preview renderer | not started |
| v0.1 | blocked on all three |

Nothing in the register has been frozen by a spike. See [`docs/DECISIONS.md`](docs/DECISIONS.md) for what has been decided and why, and [`docs/REVIEW-2026-09-05.md`](docs/REVIEW-2026-09-05.md) for what is still open.

## The governing principle

> Freeze interfaces and invariants early. Freeze implementation choices only after a spike proves them.

Two rules follow, and the project is held to both. **Every frozen item carries a written reason** — if it can't be justified in a sentence, it isn't frozen, it's a habit. **Every provisional item names the spike that resolves it** — "provisional" without an exit condition is just indecision with better PR.

### Frozen

| Item | Reason |
|---|---|
| The source file is never modified | Non-destructive is the product. Enforced by a byte-identity test on every commit |
| One shader source drives preview and export | Separate paths guarantee undiscoverable WYSIWYG drift |
| The document is declarative, versioned, human-readable | Presets, history, batch and agent editing all fall out of it for free |
| Pipeline order is explicit and versioned | It's the contract. Implicit order is unmigrateable |
| AI is removable without touching the render path | Protects the editor from the volatile part |
| Every cache is regenerable | No cache is ever load-bearing state |
| Masks read the layer's input | Unnamed, it gets chosen accidentally and differently in preview and export |
| History is session-only | A sidecar accumulating every gesture grows without bound |

### Provisional

| Item | Resolved by |
|---|---|
| Fork RapidRAW, or build against `rawler` + `libheif` directly | Spike A |
| Working space — linear Display P3, f16 | Spike B |
| Preview renderer path | Spike C |
| v1 pipeline stage ordering | Golden-image validation |
| Front-end framework | Spike A |
| v1 discards iPhone HDR gain maps | An HDR display, or the first wanted gain-mapped export |

## Phase 0

**Spike A — fork audit.** Classify every RapidRAW subsystem as `KEEP` / `ADAPT` / `REPLACE` / `AVOID`. Four gate criteria, all of which must hold, or the fork doesn't happen and RapidRAW becomes an architectural reference instead. Deliverable: `docs/FORK-AUDIT.md`.

**Spike B — colour validation harness.** Prove linear Display P3 f16 rather than assuming it, on a corpus with known values and ΔE2000 thresholds. Built standalone so it survives either fork outcome, and kept afterwards as a permanent test suite.

**Spike C — preview renderer.** Establish whether one shader source can drive both preview and export on this platform. Scouting has already found that RapidRAW's chain is compute-based, and WebGL2 has no compute shaders, no storage textures and no storage buffers — so the hand-written-GLSL fallback is the expected outcome rather than a contingency.

## Picking up

The next action is **Spike A**, and it does not need a toolchain — it is a source audit of RapidRAW 1.6.3, already unpacked at `~/Downloads/RapidRAW-main`. Classify the subsystems listed in §2.1, fill in the table, and record the gate as explicitly passed or failed. Deliverable: `docs/FORK-AUDIT.md`.

Part of that audit is already done and written into §2.1 — coupling measurements, the edit-state target, and the finding that the shader chain is compute end to end. Start from that table rather than from zero.

**Local environment, as of 2026-09-05.** Present: Rust 1.98, `lcms2-devel`, `libheif` runtime, `gh` 2.97. Absent and needed later: `libheif-devel` (Spike B), Node and npm (any Tauri front end — RapidRAW is React + Vite), and `webkit2gtk4.1-devel` / `gtk3-devel` / `librsvg2-devel` / `openssl-devel` (Tauri itself). The GPU is an AMD Cezanne Vega iGPU, which makes ROCm doubtful for §9.1's `LocalGpu` — Vulkan compute is the realistic path.

**Spike B is deliberately buildable now**, independently of Spike A's outcome, because it only needs `libheif` and `lcms2`. If Spike A stalls, that is the thing to build instead of waiting.

## Roadmap

| Version | Scope | Estimate |
|---|---|---|
| 0.0 | The three spikes. **No product.** | 3 weeks |
| 0.1 | Open a HEIF → exposure, contrast, highlights, shadows, blacks, temperature → before/after → export, colour-correct end to end. Document model, graph compile, source-preservation and golden tests running | 3 weeks |
| 0.2 | Crop, rotate, straighten. Presets, copy/paste edits. Undo/redo at gesture granularity | 2 weeks |
| 0.3 | Colour: curves, HSL, grading wheels, vibrance | 3 weeks |
| 0.4 | Masks: brush, linear, radial, luminance, colour range, composition | 4 weeks |
| 0.5 | AI: segmentation, inpaint, denoise, upscale, OCR behind a provider interface | 3 weeks |
| 0.6 | RAW prefix | 1–2 weeks |
| 0.7 | Workflow: folders, ratings, search, batch export | 4 weeks |

v0.1 deliberately excludes curves and HSL. They're the fun part, which is exactly why they get deferred — colour correctness, the document model and the test harness are the parts that are expensive to retrofit and impossible to bolt on later.

**Ship v0.1 before designing v0.4.**

## Licensing

Everything currently in this repository is original writing and carries no third-party licence obligations.

RapidRAW, the candidate fork base, is **AGPL-3.0**. A licensing review is required before any redistribution, publication or portfolio use of code derived from it, and that review is sequenced to gate Spike A's conclusion rather than shipping — the fork decision commits months of work, and this repository is public. Nothing here is legal advice.

Until that gate is decided, this repository stays specification-only.
