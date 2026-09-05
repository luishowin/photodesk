# PhotoDesk

A display-referred photo editor for Fedora / GNOME.

Its native subject is a finished image — an iPhone HEIF, a JPEG, a screenshot — that already has a rendering intent baked in by the device that made it. It is not a RAW laboratory: it doesn't reconstruct scene radiance, doesn't ask you to pick a demosaic algorithm or a view transform. RAW is an *input*, not a worldview.

The test for any feature: **does it shorten the path between opening a photo and being happy with it?**

**Status page:** https://luishowin.github.io/photodesk/

---

## Status — v0.0, Phase 0 complete

There is no application yet, and that is on purpose. The architecture spec commits to three spikes before any product code, on the argument that discovering their answers in month four is far more expensive than spending three weeks on them now. **All three have now run.**

| Phase | State |
|---|---|
| Spec | v0.10 — [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| Spike A — RapidRAW fork audit | **complete — gate failed, no fork.** [`docs/FORK-AUDIT.md`](docs/FORK-AUDIT.md) |
| Spike B — colour validation harness | **complete — green, working space frozen.** [`docs/SPIKE-B.md`](docs/SPIKE-B.md) |
| Spike C — preview renderer | **complete — green, preview path frozen.** [`docs/SPIKE-C.md`](docs/SPIKE-C.md) |
| v0.1 | **unblocked** |

**Spike A failed its gate on 2026-09-05, which is the outcome it was run to find.** RapidRAW's per-pixel chain is one compute kernel in which stage order is the literal statement order, and vendored shaders are read-only — so "pipeline order is explicit and versioned", a frozen item, could not be implemented inside the fork. Three further findings said the fork would not have supplied much of what it was wanted for: RapidRAW has **no colour management at all**, **cannot open HEIF**, and on Linux ships every preview frame as a lossy JPEG over IPC. PhotoDesk builds against `rawler` + `libheif` + its own shaders instead.

**Spike B is green.** At thirty render passes — the deepest chain the performance budget permits — f16 storage costs ΔE2000 **0.0956** against a budget of 1.0, and the deep-shadow ramp comes back bit-exact. The working space is frozen as **linear Display P3, f16**, and the f32 fallback will not be built. Harness at [`tests/color/`](tests/color) — eight tests, two of which validate the harness against lcms2 rather than the pipeline against the harness, because a colour suite that only agrees with itself is green and meaningless.

**Spike C is green, and went the other way too.** Spike A appeared to kill §7.2's "author once in WGSL, transpile for the webview" — but that reasoning was about RapidRAW's compute chain, and there is no fork. Authored fragment-first, the shader lowers to GLSL ES 3.00, WebKitGTK compiles it, six layers cost **5.92 ms at 2 MP** against a 16 ms budget, and — the number that matters for §0 — the WebGL2 and wgpu paths render the same source to **max 0.0088** across 196,608 channel samples. "One shader source, preview and export" has been frozen on an argument since the first draft; it now has a measurement.

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
| Export gamut-mapping policy | Before v0.1 exports — §4 never named one |
| How the RPM handles HEVC | Before v0.7 packaging — it cannot require RPM Fusion |
| v1 pipeline stage ordering | Golden-image validation |
| v1 discards iPhone HDR gain maps | An HDR display, or the first wanted gain-mapped export |

Five items left the table on 2026-09-05 and are now frozen: **do not fork RapidRAW**, **build against `rawler` + `libheif` + our own shaders**, **the working space is linear Display P3 at f16**, **the preview runs WebGL2 from transpiled WGSL**, and **shaders are authored fragment-first** — the last of these because naga refuses compute, storage buffers and storage textures by name, so one of them anywhere breaks the preview path everywhere.

A sixth followed on 2026-09-06: **no front-end framework** — TypeScript and Vite, zero runtime dependencies. Spike C showed the canvas needs none, and §11 specifies sliders closely enough (`Shift`-drag at 0.1× travel, one gesture per undo entry, scroll only when the control is hovered) that a component library would be overridden rather than used. Panels, undo and the keymap are hand-written, and that bill arrives at v0.2–v0.7 rather than v0.1.

## Phase 0

**Spike A — fork audit. ✅ Done, gate failed.** Every subsystem classified `KEEP` / `ADAPT` / `REPLACE` / `AVOID`, four gate criteria scored, two failed. Evidence, the subsystem table and the register consequences are in [`docs/FORK-AUDIT.md`](docs/FORK-AUDIT.md).

**Spike B — colour validation harness. ✅ Done, green.** Four tests at the thresholds §2.2 states, plus two cross-validations against lcms2. f16 costs under a tenth of the ΔE 1.0 budget at the deepest chain permitted, so the working space is frozen. Numbers in [`docs/SPIKE-B.md`](docs/SPIKE-B.md). Kept as a permanent suite — it runs in ~20 ms with no fixtures.

**Spike C — preview renderer. ✅ Done, green.** One shader source does drive both paths, and they agree to max 0.0088 across 196,608 samples. Six layers at 2 MP cost 5.92 ms against a 16 ms budget. `EXT_color_buffer_float` is present and RGBA16F is colour-renderable and linear-filterable in WebKitGTK, which retires the risk that Spikes B and C could each be green and jointly wrong. Numbers in [`docs/SPIKE-C.md`](docs/SPIKE-C.md); harness in [`tests/renderer/`](tests/renderer).

## Picking up

**Phase 0 is done and v0.1 is unblocked.** Four things stand between here and starting it, none of them a spike:

1. **Name the export gamut-mapping policy.** §4 says "linear P3 → tone encode → sRGB" and stops. The Spike B harness clips per channel in linear light, which is defensible and currently decided in a test file rather than in the spec.
2. **Confirm webkit2gtk-4.1.** Spike C ran in webkitgtk-6.0 via Epiphany. Tauri v2 binds webkit2gtk-4.1 — same WebKit 2.52.5, shared WebGL implementation, unconfirmed. It belongs to the first v0.1 build.

ICC extraction from a real container is proven across all three containers this machine can write — uncompressed, AVIF and **HEVC, the actual iPhone HEIC path**. The profile survives byte-identical, parses back to P3's red primary rather than sRGB's, and drives the transform to within ΔE 1.5 — where ignoring it costs 3.43, which is what RapidRAW does to every P3 file it opens.

One fact came out of that with consequences: **a HEIC carries about ΔE 0.9 of RGB↔YCbCr conversion loss before PhotoDesk sees a pixel.** Measured identical to four decimals across libaom and x265, both asked for lossless, so it is the conversion rather than the compression — and Apple does not ship uncompressed. §12.1's golden thresholds for HEIC sources have to clear it.

The front end is settled: **no framework**, TypeScript and Vite (§10.3).

**Local environment, as of 2026-09-05.** Present: Rust 1.98, Node 22.23 / npm 10.9, `lcms2-devel` 2.16, `libheif-devel` 1.21.2, WebKitGTK 2.52.5 (both 4.1 and 6.0), Mesa 26.1.8, `gh` 2.97. Still absent and needed for Tauri itself: `webkit2gtk4.1-devel`, `gtk3-devel`, `librsvg2-devel`, `openssl-devel`. The GPU is an AMD Cezanne Vega iGPU — wgpu reaches it through Vulkan as `RADV RENOIR`, which makes Vulkan compute rather than ROCm the realistic path for §9.1's `LocalGpu`.

**The repository is no longer specification-only.** That constraint existed because the fork decision was live and this repository is public. The decision is closed, nothing is vendored, and the code that follows is original.

## Roadmap

| Version | Scope | Estimate |
|---|---|---|
| ~~0.0~~ | ~~The three spikes.~~ **Complete** | 3 weeks est., 1 day actual |
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

Everything in this repository is original. The colour harness at `tests/color/` links `lcms2` (MIT) as a dev-dependency for cross-validation only, and `tests/renderer/` uses `naga` and `wgpu` (both MIT/Apache-2.0), which are on the shipping path by design.

RapidRAW is **AGPL-3.0**. The licensing review was sequenced to gate Spike A's conclusion, because the fork decision commits months of work and this repository is public. **The engineering gate failed first, so no code derived from RapidRAW exists or will** — nothing is vendored, adapted or redistributed. `docs/FORK-AUDIT.md` quotes identifiers and line numbers for the purpose of the audit and copies no source. "Architectural reference" means reading their code and then writing ours, which is a distinct question and worth raising if a review still happens. Nothing here is legal advice.
