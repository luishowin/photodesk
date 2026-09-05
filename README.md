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
| Spec | v0.5 — [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| Spike A — RapidRAW fork audit | **complete — gate failed, no fork.** [`docs/FORK-AUDIT.md`](docs/FORK-AUDIT.md) |
| Spike B — colour validation harness | not started — **unblocked, and now the critical path** |
| Spike C — preview renderer | not started |
| v0.1 | blocked on B and C |

**Spike A failed its gate on 2026-09-05, which is the outcome it was run to find.** RapidRAW's per-pixel chain is one compute kernel in which stage order is the literal statement order, and vendored shaders are read-only — so "pipeline order is explicit and versioned", a frozen item, could not be implemented inside the fork. Three further findings said the fork would not have supplied much of what it was wanted for: RapidRAW has **no colour management at all**, **cannot open HEIF**, and on Linux ships every preview frame as a lossy JPEG over IPC. PhotoDesk builds against `rawler` + `libheif` + its own shaders instead.

See [`docs/DECISIONS.md`](docs/DECISIONS.md) for what has been decided and why, and [`docs/REVIEW-2026-09-05.md`](docs/REVIEW-2026-09-05.md) for what is still open.

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
| Working space — linear Display P3, f16 | Spike B |
| Preview renderer path | Spike C |
| v1 pipeline stage ordering | Golden-image validation |
| Front-end framework | Spike C — Spike A's exit is spent, and there is no fork to inherit one from |
| v1 discards iPhone HDR gain maps | An HDR display, or the first wanted gain-mapped export |

Two items left the table on 2026-09-05 and are now frozen: **do not fork RapidRAW**, and **build against `rawler` + `libheif` + our own shaders**.

## Phase 0

**Spike A — fork audit. ✅ Done, gate failed.** Every subsystem classified `KEEP` / `ADAPT` / `REPLACE` / `AVOID`, four gate criteria scored, two failed. Evidence, the subsystem table and the register consequences are in [`docs/FORK-AUDIT.md`](docs/FORK-AUDIT.md).

**Spike B — colour validation harness.** Prove linear Display P3 f16 rather than assuming it, on a corpus with known values and ΔE2000 thresholds. Kept afterwards as a permanent test suite. It was always buildable independently of Spike A; it is now also the thing most worth building, because the fork base turned out to have no colour management of any kind and §4 is greenfield rather than an adaptation.

**Spike C — preview renderer.** Establish whether one shader source can drive both preview and export on this platform. Spike A reopened this: the compute chain that had no GLSL ES 3.0 target to lower onto was RapidRAW's, and we are not forking it. Writing our own shaders fragment-first avoids the constructs GLSL lacks, so `naga` transpilation is a live branch again — one the spike now has to test rather than assume, in either direction.

## Picking up

The next action is **Spike B**, the colour validation harness. It is now the critical path rather than the parallel track: Spike A established that there is no incumbent colour architecture to adapt, so §4 gets built from nothing and the harness is what decides whether it is built on f16.

It needs `libheif-devel`, which is **not installed** — `sudo dnf install libheif-devel` is the first command. `lcms2-devel` is present. Nothing else in Spike B needs a webview, Node, or Tauri.

**Local environment, as of 2026-09-05.** Present: Rust 1.98, `lcms2-devel`, `libheif` runtime, `gh` 2.97. Absent and needed: `libheif-devel` (Spike B, immediately), Node and npm (Spike C's WebGL2 harness and any front end), and `webkit2gtk4.1-devel` / `gtk3-devel` / `librsvg2-devel` / `openssl-devel` (Tauri itself). The GPU is an AMD Cezanne Vega iGPU, which makes ROCm doubtful for §9.1's `LocalGpu` — Vulkan compute is the realistic path.

**The repository is no longer specification-only.** That constraint existed because the fork decision was live and this repository is public. The decision is closed, nothing is vendored, and the code that follows is original.

## Roadmap

| Version | Scope | Estimate |
|---|---|---|
| 0.0 | The three spikes. **No product.** | 3 weeks — A done |
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

RapidRAW is **AGPL-3.0**. The licensing review was sequenced to gate Spike A's conclusion, because the fork decision commits months of work and this repository is public. **The engineering gate failed first, so no code derived from RapidRAW exists or will** — nothing is vendored, adapted or redistributed. `docs/FORK-AUDIT.md` quotes identifiers and line numbers for the purpose of the audit and copies no source. "Architectural reference" means reading their code and then writing ours, which is a distinct question and worth raising if a review still happens. Nothing here is legal advice.
