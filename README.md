# PhotoDesk

A display-referred photo editor for Fedora / GNOME.

Its native subject is a finished image — an iPhone HEIF, a JPEG, a screenshot — that already has a rendering intent baked in by the device that made it. It is not a RAW laboratory: it doesn't reconstruct scene radiance, doesn't ask you to pick a demosaic algorithm or a view transform. RAW is an *input*, not a worldview.

The test for any feature: **does it shorten the path between opening a photo and being happy with it?**

**Status page:** https://luishowin.github.io/photodesk/

---

## Status — v0.1 runs

There is an application. The architecture spec committed to three spikes before any product code, on the argument that discovering their answers in month four is far more expensive than spending three weeks on them now — **all three ran**, and v0.1 opens a photograph, edits it and exports it.

| Phase | State |
|---|---|
| Spec | v0.20 — [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| Spike A — RapidRAW fork audit | **complete — gate failed, no fork.** [`docs/FORK-AUDIT.md`](docs/FORK-AUDIT.md) |
| Spike B — colour validation harness | **complete — green, working space frozen.** [`docs/SPIKE-B.md`](docs/SPIKE-B.md) |
| Spike C — preview renderer | **complete — green, preview path frozen.** [`docs/SPIKE-C.md`](docs/SPIKE-C.md) |
| v0.1 | **runs** — open a photograph, drag six sliders, compare against the original, export |

**Spike A failed its gate on 2026-09-05, which is the outcome it was run to find.** RapidRAW's per-pixel chain is one compute kernel in which stage order is the literal statement order, and vendored shaders are read-only — so "pipeline order is explicit and versioned", a frozen item, could not be implemented inside the fork. Three further findings said the fork would not have supplied much of what it was wanted for: RapidRAW has **no colour management at all**, **cannot open HEIF**, and on Linux ships every preview frame as a lossy JPEG over IPC. PhotoDesk builds against `rawler` + `libheif` + its own shaders instead.

**Spike B is green.** At thirty render passes — the deepest chain the performance budget permits — f16 storage costs ΔE2000 **0.0956** against a budget of 1.0, and the deep-shadow ramp comes back bit-exact. The working space is frozen as **linear Display P3, f16**, and the f32 fallback will not be built. Harness at [`tests/color/`](tests/color) — eight tests, two of which validate the harness against lcms2 rather than the pipeline against the harness, because a colour suite that only agrees with itself is green and meaningless.

**Spike C is green, and went the other way too.** Spike A appeared to kill §7.2's "author once in WGSL, transpile for the webview" — but that reasoning was about RapidRAW's compute chain, and there is no fork. Authored fragment-first, the shader lowers to GLSL ES 3.00, WebKitGTK compiles it, six layers cost **5.92 ms at 2 MP** against a 16 ms budget, and — the number that matters for §0 — the WebGL2 and wgpu paths render the same source to **max 0.0088** across 196,608 channel samples. "One shader source, preview and export" has been frozen on an argument since the first draft; it now has a measurement.

**And the export now has a policy, which §4 never gave it.** A Display P3 photograph exported to sRGB produces channels the smaller gamut cannot hold, and something has to decide what happens to them; until 2026-09-06 that something was one `clamp` in a test harness, worth up to **ΔE 2.86 on real photographs**. The measurement that settled it had to be built around a trap: clamping each channel to [0,1] *is* the Euclidean projection onto the gamut cube, so the clip is the nearest in-gamut colour — in linear RGB, a space nobody perceives in — and every distance-minimising policy puts all 81 of 81 out-of-gamut samples on the gamut surface, which is exactly how a gradient becomes a flat patch. **The lowest ΔE and the flattest gradient are the same answer**, so ΔE cannot rank these and the harness ranks them on what the error is made of instead. Frozen: **clip chroma at constant luminance**, which holds L\* exact by construction and halves the adjacent pixel pairs a real photograph merges into one colour, at zero cost inside the gamut.

**The preview path is confirmed in the webview Tauri actually embeds.** Spike C ran in Epiphany, which is webkitgtk-6.0; Tauri v2 binds webkit2gtk-4.1, and the report left that gap to the first v0.1 build. Driving the 4.1 WebView directly closed it a day later: identical capabilities, an identically compiled shader, and pixel agreement **identical to every digit**.

**v0.1's first piece is the document model** ([`src-tauri/src/photodesk/`](src-tauri/src/photodesk)), which §14 puts before the sliders on the argument that the boring parts are the ones impossible to bolt on later. §6.3's migration table is six conditions with six different behaviours and three of them are not accept-or-reject, so it lives in the types: loading returns a `Loaded` whose document is not a public field, and since the only door to an owned one is `into_writable()`, **read-only is enforced by the compiler** rather than by everyone's care. The schema is defined once in Rust and the TypeScript types are *generated* from it, for the same reason the browser harness loads generated GLSL rather than a hand-written twin.

Writing it found a bug in the specification, in the one workflow §14 gives v0.1. §6.1 named the sidecar `IMG_4821.photodesk.json` — extension dropped. But v0.1 is "open a HEIF → … → export", so the export lands as `IMG_4821.jpg` beside `IMG_4821.HEIC`, and under that naming the two photographs share one sidecar: editing the export silently overwrites the original's edits. The sidecar now keeps the whole filename.

**Everything headless in v0.1 now exists**: the document model, §6.2's graph compile, decode, the render path through wgpu, and export. §12.3's source-preservation test has all five of its steps for the first time, and §12.2's proxy/full-res agreement is measured — as a *precondition* rather than a number, because the two paths agree to 0.15 of an 8-bit code on band-limited in-gamut content and to 152 on content finer than the proxy, and neither figure is a defect.

The last format gap closed on 2026-09-06: **PNG, and therefore screenshots.** §1 has named a screenshot a native subject since the first draft and §4 wrote it a colour rule, but the decoder read HEIF and JPEG only — so "assume sRGB if untagged" was being demonstrated against an untagged JPEG. The colour half was small, as expected; what PNG actually brought was a container with opinions — palette, sub-byte samples, greyscale, sixteen-bit, Adam7, alpha — and the decision that mattered was resolving all of them *before* §4's chain rather than as branches inside it. Alpha is the one that is a judgement call: it is composited onto white, in linear light, because dropping it is not the neutral option it looks like — the RGB under a transparent pixel is whatever the compositor last wrote there, so a window screenshot's rounded corners would arrive carrying arbitrary colour.

It also closed a half-claim. `export` has written PNG since v0.1's export landed, and nothing could read the `iCCP` it deflates back; PNG is the only format in v0.1 where a round trip is an equality rather than a tolerance, and a photograph now goes out through the exporter and back in through the decoder to within **0.00293** — under the ~0.004 that one 8-bit code is worth at that part of the curve.

**And then a window, which is the first part a person touches.** §14's v0.1 is "open a HEIF → six sliders → before/after → export, colour correct end to end", and the last clause of that was unreachable until there was a front end. Tauri, WebGL2, and §11's slider written by hand — §10.3 accepted that bill knowingly, on the argument that a component library's slider does none of `Shift` = 0.1× travel, `Ctrl` = 10×, double-click to reset, scroll only when hovered, and one-gesture-one-undo-entry, "so it would be overridden rather than used". Zero runtime dependencies, and the bundle is 21 kB.

The window is a **separate crate** from the library, which is the whole shape of the arrangement: `photodesk` has no `tauri` dependency and now never will, because §12.3's source preservation and §12.1's golden images have to run headless on every commit and a crate that links webkit cannot. An optional feature would have been tidier and would have left `--all-features` able to break that quietly.

**§12.2 is now measured across the boundary it was written for.** Spike C proved the *path* with a stress shader and a hand-written page; this is the product. The front end's own modules — `src/graph/execute.ts` and `src/canvas/gl.ts`, the ones the app imports — run inside webkit2gtk-4.1, the binding Tauri embeds, over inputs Rust emitted, and are compared against wgpu's render of the same compiled plan. They agree to **max 0 of 255 across 12,288 channels**: not close, identical after quantisation, with all nine of the document's parameters bound by name. The presented canvas was then checked once against the same reference and is pixel-identical to it — and 168 codes away from it upside down, which is the measurement that pins the one step the harness cannot reach.

That last point is not pedantry. WGSL is authored for wgpu, whose framebuffer origin is top-left; GL's is bottom-left, so **every preview pass inverts the image**, and the pass count changes with the number of layers. It is resolved once at a blit rather than by convention — which also makes fit-to-window free and the before/after split a second blit rather than a second shader.

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
| Export clips chroma at constant luminance | The per-channel clip's error has no policy — how much lightness is lost depends on which channel ran out first |

### Provisional

| Item | Resolved by |
|---|---|
| How the RPM handles HEVC | Before v0.7 packaging — it cannot require RPM Fusion |
| v1 pipeline stage ordering | Golden-image validation |
| v1 discards iPhone HDR gain maps | An HDR display, or the first wanted gain-mapped export |
| Golden-image thresholds | Before the first `--bless` — they have to clear both a ~0.9 ΔE YCbCr floor and one colour-attachment step |

Five items left the table on 2026-09-05 and are now frozen: **do not fork RapidRAW**, **build against `rawler` + `libheif` + our own shaders**, **the working space is linear Display P3 at f16**, **the preview runs WebGL2 from transpiled WGSL**, and **shaders are authored fragment-first** — the last of these because naga refuses compute, storage buffers and storage textures by name, so one of them anywhere breaks the preview path everywhere.

A sixth followed on 2026-09-06: **no front-end framework** — TypeScript and Vite, zero runtime dependencies. Spike C showed the canvas needs none, and §11 specifies sliders closely enough (`Shift`-drag at 0.1× travel, one gesture per undo entry, scroll only when the control is hovered) that a component library would be overridden rather than used. Panels, undo and the keymap are hand-written, and that bill arrives at v0.2–v0.7 rather than v0.1.

A seventh on the same day: **the export gamut policy**, above. It came with a constraint worth naming, because it is the shape of the finding that killed the fork asked in advance for once — the output encode is the one stage every pixel of *both* paths goes through, so a policy that cannot be expressed in a fragment shader is not adoptable whatever its colorimetry. The chosen one is a matrix multiply, a dot product, three divides and a min; it lowers to GLSL ES 3.00 and agrees with its Rust reference bit-exactly on 48,020 of 49,152 channels. The soft-knee variant that keeps gradients slightly better was rejected for moving 3.23% of an already-correct frame on the strength of a tuning constant with no derivation behind it.

## Phase 0

**Spike A — fork audit. ✅ Done, gate failed.** Every subsystem classified `KEEP` / `ADAPT` / `REPLACE` / `AVOID`, four gate criteria scored, two failed. Evidence, the subsystem table and the register consequences are in [`docs/FORK-AUDIT.md`](docs/FORK-AUDIT.md).

**Spike B — colour validation harness. ✅ Done, green.** Four tests at the thresholds §2.2 states, plus two cross-validations against lcms2. f16 costs under a tenth of the ΔE 1.0 budget at the deepest chain permitted, so the working space is frozen. Numbers in [`docs/SPIKE-B.md`](docs/SPIKE-B.md). Kept as a permanent suite — it runs in ~20 ms with no fixtures.

**Spike C — preview renderer. ✅ Done, green.** One shader source does drive both paths, and they agree to max 0.0088 across 196,608 samples. Six layers at 2 MP cost 5.92 ms against a 16 ms budget. `EXT_color_buffer_float` is present and RGBA16F is colour-renderable and linear-filterable in WebKitGTK, which retires the risk that Spikes B and C could each be green and jointly wrong. Numbers in [`docs/SPIKE-C.md`](docs/SPIKE-C.md); harness in [`tests/renderer/`](tests/renderer). Re-run on 2026-09-06 in **webkit2gtk-4.1** — `python3 tests/renderer/web/run-probe.py --engine webkit2gtk-4.1` — with identical results.

## Picking up

**v0.1 runs, so what is next is v0.2** — crop, rotate, straighten; presets; undo/redo at gesture granularity. Undo already commits per gesture; what v0.2 adds is the panel and the geometry node, which the plan executor currently refuses by name.

One thing worth doing before it, and it is not a measurement: **the six sliders have never been dragged over a real photograph by a person.** Everything here is measured, and measurement is not use.

Four things stood in front of v0.1 and none does now:

1. ~~**Name the export gamut-mapping policy.**~~ Closed — clip chroma at constant luminance, measured rather than picked. [`tests/color/tests/gamut_policy.rs`](tests/color/tests/gamut_policy.rs), §4, and the entry in [`docs/DECISIONS.md`](docs/DECISIONS.md).
2. ~~**Confirm webkit2gtk-4.1.**~~ Closed — the 4.1 binding returns identical capabilities and identical pixel agreement to webkitgtk-6.0, on the same machine on the same day.
3. ~~**PNG input (§16 #17).**~~ Closed — §1's screenshot is a file the app can open, and the exporter's PNG has a reader.
4. ~~**A window.**~~ Closed — and with it the discovery that nothing had ever lowered the product's own fused pass to GLSL, because the spike lowers its stress shader and wgpu never goes through GLSL at all. It lowers. The way that would have failed is a preview that will not start, found on a first run rather than by a test.

Two findings arrived alongside them, both of which land on the golden-image suite before its first `--bless`. **An RGBA16F colour attachment can truncate toward zero rather than round to nearest** — measured on RADV/RENOIR, where 48,020 of 49,152 stored channels are bit-exactly the reference truncated — which is a full-step bias that is the driver's doing and not the shader's. And a green test can quietly change what it measures: freezing the gamut policy took the colour harness's P3→sRGB test from mean ΔE 0.0807 to **1.4183 against its own 1.5 threshold**, still passing and no longer about transform fidelity at all, because its reference converter clips and the pipeline no longer did.

ICC extraction from a real container is proven across all three containers this machine can write — uncompressed, AVIF and **HEVC, the actual iPhone HEIC path**. The profile survives byte-identical, parses back to P3's red primary rather than sRGB's, and drives the transform to within ΔE 1.5 — where ignoring it costs 3.43, which is what RapidRAW does to every P3 file it opens.

One fact came out of that with consequences: **a HEIC carries about ΔE 0.9 of RGB↔YCbCr conversion loss before PhotoDesk sees a pixel.** Measured identical to four decimals across libaom and x265, both asked for lossless, so it is the conversion rather than the compression — and Apple does not ship uncompressed. §12.1's golden thresholds for HEIC sources have to clear it.

**§4 has now been checked against real photographs**, an iPhone HEIC and an iPhone JPEG. Both tag Display P3 with the same 536-byte profile; the base image is 8-bit; a real photograph round-trips through linear P3 f16 at **max ΔE 0.0000**. The HDR gain map is present as a half-resolution auxiliary image under `urn:com:apple:photo:2020:aux:hdrgainmap`, and libheif's default decode returns the SDR base without applying it — so v1's discard is now a skip that can be seen rather than one assumed. The photographs are not in this repository; the test reads from `PHOTODESK_CORPUS_DIR` and skips when it is empty, which is what it does on this machine today — put a photograph back and the real-file cases in both [`tests/color/tests/real_photos.rs`](tests/color/tests/real_photos.rs) and `gamut_policy.rs` run again.

The front end is settled: **no framework**, TypeScript and Vite (§10.3).

**Local environment, as of 2026-09-06.** Present: Rust 1.98, Node 22.23 / npm 10.9, `lcms2-devel` 2.16, `libheif-devel` 1.21.2 with `libheif-freeworld` for HEVC, WebKitGTK 2.52.5 (both 4.1 and 6.0, **both now measured**), Mesa 26.1.8, `gh` 2.97. Still absent and needed for Tauri itself: `webkit2gtk4.1-devel`, `gtk3-devel`, `librsvg2-devel`, `openssl-devel`. The GPU is an AMD Cezanne Vega iGPU — wgpu reaches it through Vulkan as `RADV RENOIR`, which makes Vulkan compute rather than ROCm the realistic path for §9.1's `LocalGpu`.

**The repository is no longer specification-only.** That constraint existed because the fork decision was live and this repository is public. The decision is closed, nothing is vendored, and the code that follows is original.

## Roadmap

| Version | Scope | Estimate |
|---|---|---|
| ~~0.0~~ | ~~The three spikes.~~ **Complete** | 3 weeks est., 1 day actual |
| ~~0.1~~ | ~~Open a HEIF → exposure, contrast, highlights, shadows, blacks, temperature → before/after → export, colour-correct end to end.~~ **Runs.** Golden tests are §16 #14/#15, still open | 3 weeks est. |
| 0.2 | Crop, rotate, straighten. Presets, copy/paste edits. Undo/redo at gesture granularity | 2 weeks |
| 0.3 | Colour: curves, HSL, grading wheels, vibrance | 3 weeks |
| 0.4 | Masks: brush, linear, radial, luminance, colour range, composition | 4 weeks |
| 0.5 | AI: segmentation, inpaint, denoise, upscale, OCR behind a provider interface | 3 weeks |
| 0.6 | RAW prefix | 1–2 weeks |
| 0.7 | Workflow: folders, ratings, search, batch export | 4 weeks |

v0.1 deliberately excludes curves and HSL. They're the fun part, which is exactly why they get deferred — colour correctness, the document model and the test harness are the parts that are expensive to retrofit and impossible to bolt on later.

**Ship v0.1 before designing v0.4.**

## Running what exists

```sh
npm install && npm run build          # the front end; dist/ is what the window loads
cargo run -p photodesk-app [path]     # a path opens straight into the editor
```

Building the window needs four devel packages: `webkit2gtk4.1-devel`, `gtk3-devel`, `librsvg2-devel`, `openssl-devel`. `npm run dev` on its own does not work and says so — without the Rust core there is nothing to decode a photograph.

`src-tauri/` is still a library that links no webview, deliberately, so that §12.3's source-preservation test and §12.1's golden images run headless on every commit. The two Phase 0 harnesses are permanent suites rather than scaffolding: §13 keeps them because every later golden-image test sits on top of what they check.

```sh
cargo test --workspace                                   # everything, 131 tests, ~4 s
cargo test -p photodesk -- --nocapture                   # the core: document, graph, decode, render, export
cargo test -p photodesk-color -- --nocapture             # the colour numbers
cargo test -p photodesk-renderer-spike -- --nocapture    # transpilation and agreement
```

`--nocapture` is not optional if you want to know anything: the numbers are the deliverable and the assertions only say they were in range.

The browser half of Spike C needs the generated GLSL, which `cargo test -p photodesk-renderer-spike` emits into a gitignored directory — the browser loads the *transpiled* shader rather than a hand-written twin, deliberately, because a twin would make the one-shader-source invariant untestable.

```sh
python3 tests/renderer/web/run-probe.py                          # webkitgtk-6.0, via Epiphany
python3 tests/renderer/web/run-probe.py --engine webkit2gtk-4.1  # what Tauri v2 embeds
```

§12.2's front-end agreement is the same machinery with `--plan`, and it needs the bundle as well as the fixtures:

```sh
cargo test -p photodesk-renderer-spike
npm run build:harness
python3 tests/renderer/web/run-probe.py --engine webkit2gtk-4.1 --plan
```

`cargo test -p photodesk` also regenerates `src/document/generated/document.ts` and fails if it moved, so a schema change is: edit the Rust, run the tests once, commit both.

Tests that need a real photograph read `PHOTODESK_CORPUS_DIR` (default `~/Downloads`) and skip with an explanation when it holds none. The photographs are personal files and are not in this repository.

## Licensing

Everything in this repository is original. The colour harness at `tests/color/` links `lcms2` (MIT) as a dev-dependency for cross-validation only, and `tests/renderer/` uses `naga` and `wgpu` (both MIT/Apache-2.0), which are on the shipping path by design.

RapidRAW is **AGPL-3.0**. The licensing review was sequenced to gate Spike A's conclusion, because the fork decision commits months of work and this repository is public. **The engineering gate failed first, so no code derived from RapidRAW exists or will** — nothing is vendored, adapted or redistributed. `docs/FORK-AUDIT.md` quotes identifiers and line numbers for the purpose of the audit and copies no source. "Architectural reference" means reading their code and then writing ours, which is a distinct question and worth raising if a review still happens. Nothing here is legal advice.
