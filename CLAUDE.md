# PhotoDesk — working notes

A display-referred photo editor for Fedora / GNOME. Read `docs/ARCHITECTURE.md` before proposing anything; it is the master spec and it is unusually prescriptive on purpose.

## How this project works

The spec's §0 register is the contract. Every item is either **FROZEN** (with a written reason) or **PROVISIONAL** (with a named exit condition). Anything not in the register is undecided, and a decision must be recorded there before code depends on it.

Two rules govern changes to it:

- **Don't freeze without a reason you can write in a sentence.** If it can't be justified, it isn't frozen, it's a habit.
- **Don't mark something provisional without naming its exit condition.** "Provisional" with no exit is indecision with better PR.

When something moves between states, append to `docs/DECISIONS.md` — what moved, what resolved it, the date. The register is the current state; that file is the history, and it is where the *reasoning* lives. **These notes are a map, not a log** — when you want to know why something is the way it is, the DECISIONS entry for the day it changed will tell you at length.

Don't edit past DECISIONS entries.

## Current state

**Spec v0.20. Phase 0 complete; v0.1 runs, and has now been looked at.** 134 tests, everything on `main` and pushed. (`cargo test` prints two `failed to parse serde attribute` warnings from ts-rs; both are benign — the `ts(type = "string")` override already emits what they would have, and `deny_unknown_fields` has no TypeScript meaning.)

Phase 0's three spikes ran on 2026-09-05 and two of the three answers were not the expected ones — Spike A's gate **failed** (no fork), B and C are green. Their reports are `docs/{FORK-AUDIT,SPIKE-B,SPIKE-C}.md`. Read `FORK-AUDIT.md` before revisiting anything about the render path.

v0.1's headless half is built. What remains is the part a person touches.

| | state |
|---|---|
| Document model (§6.1, §6.3) | done — schema, validation, migration, sidecar IO |
| Graph compile (§6.2) | done — Merkle-keyed DAG, dedup, dirty tracking |
| Decode (§5 stage 0) | done — HEIF + JPEG + PNG → linear P3 f16, ICC classified |
| Render (§7.2) | done — a compiled graph through wgpu, six sliders |
| Export (§6.1's `output`) | done — JPEG + PNG, ICC-tagged, metadata policy |
| **Front end** | **done** — Tauri window, WebGL2 preview, §11's six sliders, before/after, export |
| Crop / masks / curves | v0.2–v0.4, refused by name where reachable |

**The measured paths were all correct and the application still showed a black canvas**, because the one step nothing measured was the shape the proxy arrives in. See the traps below; the lesson is that a harness feeding itself `fetch()` cannot check what Tauri's IPC hands over, and only running the real thing could.

**The second session of running it found three more of the same shape.** Every photograph was presented **upside down** — `preview.ts` derived a flip from the pass count on the theory that each pass inverts, which naga's `ADJUST_COORDINATE_SPACE` already settles, so the rule was right only for an even count and an unedited photograph is one pass. Opening a 12 MP photograph took **9.2 seconds**, 6.8 of them serialising the proxy as a JSON array of 97 million numbers, because the app's own CSP had no `connect-src ipc:` and Tauri had silently fallen back off its fast transport. And §1's native subject, a HEIC, had still never been opened. All three are fixed; `DECISIONS.md`'s second 2026-09-07 entry is the long version.

**The instrument that found two of them in one capture.** A Tauri window cannot be screenshotted from a shell on Wayland, but `GDK_BACKEND=x11` makes it an XWayland client and then `import -window PhotoDesk shot.png` works. That is the whole apparatus, and it is the difference between reasoning about the window and looking at it.

**§12.2 and §12.3 both run, and §12.2 now runs across the boundary it was written for.** Source preservation has all five of its steps; proxy/full-res agreement is measured with a *precondition* rather than a number (see below); and the front end's own modules, in Tauri's own webview, render the product's shaders **identically to wgpu — max 0 of 255 over 12,288 channels**. It measures **two** plans, an even pass count and an odd one, since the even case alone cannot tell a correct orientation from an inverted one — see the traps.

```
cargo test -p photodesk-renderer-spike        # emits the inputs and the reference
npm run build:harness                         # bundles src/graph + src/canvas for the page
python3 tests/renderer/web/run-probe.py --engine webkit2gtk-4.1 --plan
```

That is three commands rather than one because the browser half cannot run in `cargo test`. Run it after touching a shader, `src/graph/`, or `src/canvas/`.

### Running it

```
npm install && npm run build          # the front end; dist/ is what the window loads
cargo run -p photodesk-app [path]     # a path opens straight into the editor
```

`cargo run`, not `./target/…/photodesk-app` — the front end is baked into the binary, so
running it directly after a front-end edit silently runs the old one. And to see the
window from a shell:

```
cargo run -p photodesk-color --example make-heic -- /tmp/scene.heic   # something to open
GDK_BACKEND=x11 cargo run -p photodesk-app -- /tmp/scene.heic &       # XWayland, so it can be captured
import -window PhotoDesk shot.png
```

The four devel packages are installed. **`npm run build` is not optional and there is no dev-server mode** — `tauri.conf.json` has no `devUrl`, deliberately: with one, a debug build loads `http://localhost:1420` and a release build loads `dist/`, which is two ways to run the same application, one of them failing with "Connection refused" unless a second process happens to be running. `vite build` takes 200 ms for a 21 kB bundle, so what a dev server actually buys here is a mode that can be wrong. It cost one broken first run to find that out.

`npm run dev` still serves the page for looking at chrome and layout without a photograph; `ipc.ts` says so in a sentence rather than failing with a `TypeError`.

### What has actually been run

Worth its own list, because this session's lesson is that a green suite and a working
application are different claims (see the black canvas, below).

**Verified by measurement.** Decode, graph compile, render and export headless (134
tests). The front end's own modules rendering identically to wgpu inside Tauri's webview
— max 0 of 255 over 12,288 channels, at an odd pass count and an even one. And the
presented canvas containing the photograph rather than nothing, which is
`preview.diagnose()`'s centre pixel and the reason it is kept.

**Verified by a synthetic drag** in a mocked-IPC harness: a slider moves, the document
changes, the picture follows.

**Verified by looking at it.** A JPEG and a HEIC — both branches of §4's HEIF colour
reading, ICC and NCLX — open, land the right way up, and report the right space in the
readout. Screenshots via the `GDK_BACKEND=x11` route above; `make-heic` builds the HEIC.
Open time is on the `opened …` line now: 1.9 s for a 12 MP JPEG, 2.3 s for a 12 MP HEIC,
0.8 s of which is starting the process and the webview.

**Never run, by a test or by a person.** These are the paths to be suspicious of:

- **`export_image` and `save_sidecar`.** No test invokes either, and nobody has pressed
  `Ctrl+E`. The Rust underneath both is well tested; the command wrappers and the file
  dialogs around them are not, and the last bug lived in exactly that layer.
- **`Space` and `\`** — hold-for-original and the before/after split, including whether
  the seam following the pointer is right (§11 specifies the first and is silent on the
  second). The *plan* behind `Space` is now measured (`tests/renderer/`'s second case is
  the empty stack); what nobody has done is hold the key.
- **`Ctrl+Z` / `Ctrl+Shift+Z`** through the UI.

There is no way to drive the window's keyboard from a shell here — no `xdotool`, no
`ydotool`, no python-xlib — so the four above want either a person or one
`sudo dnf install xdotool`, after which the XWayland route already used for screenshots
would reach them.

### What's next

**v0.2 — crop, rotate, straighten; presets; undo/redo at gesture granularity.** Undo already commits per gesture (`src/document/history.ts`); what v0.2 adds is the panel and the geometry node, which `src/graph/execute.ts` currently refuses **by name**.

Open and worth doing before it:

- **Use it.** Still the first item, and still worth it — three real bugs came out of two sessions of it. What is left is the half a shell cannot reach: §11's feel — 0.1× travel on `Shift`, where the numeric entry lands, whether the split handle should be sticky — is a thing to sit with rather than assert, and `Ctrl+E`, `Ctrl+Z`, `Space` and `\` want one pass by hand before v0.2 builds on them.
- **The window gives the photograph under half its height.** `app.css` is `grid-template-rows: auto 1fr auto auto auto`, so the stage is one row and the panel, readout and notices stack under it: at 1440×900 the photograph gets 617×411 and the panel band keeps a third of its width empty. §10 does not specify a layout, so this is an open question rather than a bug — but a right-hand panel is the obvious alternative and it is worth deciding before v0.2 adds a second tab's worth of controls.
- **§16 #7's icon** is a placeholder, marked as one.
- **§16 #16** — the register's UI half now exists in `src/panels/light.ts`; whether the *file* has an opinion about ranges is still open, due before v0.2's presets.
- #18 (tiled export) and #19 (HEIF output) still wait.

## Where the code is

```
app/                        the window. `tauri`, the IPC commands, and no logic a test
│                           would want. A separate crate so `photodesk` stays linkable
└── src/main.rs             without webkit — a feature flag would have left
                            `--all-features` able to break that quietly

src-tauri/                  the Rust core. A library — **no `tauri` dependency, ever**:
│                           §12.1 and §12.3 must run headless, which is criterion 1 of
│                           the gate Spike A failed RapidRAW on
├── engine/                 the render core (§13)
│   ├── colour.rs           spaces, curves, matrices — all derived from chromaticities
│   ├── icc.rs              read and write a profile; §4's two spaces or a named error
│   ├── decode.rs           §5 stage 0: a file becomes linear P3 f16
│   ├── glsl.rs             §7.2's preview half: WGSL → GLSL ES 3.00, at startup
│   ├── image.rs            the working buffer, §7.1's proxy, EXIF orientation
│   ├── gamut.rs            §16 #11's export policy
│   ├── render.rs           executes a compiled graph through wgpu
│   ├── exif.rs             what a photograph says about itself; §6.1's policy
│   └── export.rs           §6.1's `output` block: a frame becomes a file
└── photodesk/              document → engine bridge (§13)
    ├── document/           the schema: model, validation, migration
    ├── graph/              §6.2's DAG compile and dirty tracking
    └── sidecar.rs          where the document sits on disk, and how it is written

shaders/photodesk/          the only shader source (§13). adjust.wgsl is §5 stages 2–9
                            fused; encode.wgsl is stage 13 with the gamut policy
src/                        the front end (§10.3: no framework, TypeScript + Vite,
├── ipc.ts                  zero runtime dependencies — `dependencies` is absent
├── canvas/                 the viewport: WebGL2, fit, before/after, the blit
├── graph/execute.ts        executes the compiled plan. It does not build one
├── panels/                 §11's slider, and the Light tab's six controls
├── design/tokens.css       §10.1's table, verbatim
└── document/generated/     TypeScript types, emitted from the Rust schema, committed
tests/color/                Spike B's harness, kept permanent. Tests the product now
└── examples/make-heic.rs   builds a HEIC to open by hand — §1's subject, on a machine
                            that has no phone. Needs libheif-freeworld; names it if not
tests/renderer/             Spike C's harness, kept permanent
```

`cargo test --workspace` runs everything in a few seconds — `photodesk` is 88 tests, `photodesk-color` 26, `photodesk-renderer-spike` 8. The browser half of Spike C runs on demand: `python3 tests/renderer/web/run-probe.py [--engine webkit2gtk-4.1]`.

## Things that will bite

**Process**

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling.
- **`FORK-AUDIT.md`, `SPIKE-B.md` and `SPIKE-C.md` are spike reports, not living documents.** They record what was measured on 2026-09-05. If something in one turns out wrong, that is a `DECISIONS.md` entry.
- **The front end is embedded in the binary, so `npm run build` alone changes nothing that runs.** `generate_context!` bakes `dist/` in at compile time. `cargo run -p photodesk-app` rebuilds and is therefore correct; running `./target/release/photodesk-app` directly — the obvious thing when iterating — silently runs the *previous* front end. The symptom is perfect: log lines you added never appear, ones that were already there do, and nothing errors. The decisive test is to change an **existing** message and see whether the change shows up.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page, updated **on request only** — do not regenerate it as a side effect of other work. **It is very stale**: spec v0.4, all three spikes open, the licensing review still gating Spike A. Fourteen spec versions and all of v0.1 behind.

**One source, never two**

This is the project's recurring shape, and it has now decided five things:

- **Never author a compute shader, a storage buffer or a storage texture.** Frozen. naga refuses all three by name targeting GLSL ES 3.00, so one of them anywhere breaks the preview path for the whole project. `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns.
- **There is no CPU renderer and there must not be one.** The obvious way to test a GPU renderer is to write the same maths in Rust and compare — precisely the drift §0 freezes against. The shaders are the only description of what a pixel goes through. The two constants hardcoded in `adjust.wgsl` (linear P3's luma weights and XYZ matrix) are the exception, and a test reads them out of the shader text and compares them against the derived values.
- **`src/document/generated/{document,graph}.ts` are generated and committed.** Don't hand-edit them, and don't hand-write a second TypeScript description of either. `cargo test -p photodesk` regenerates and fails if they changed: change the Rust, run the tests once, commit both.
- **The preview's GLSL is lowered at startup, never generated to disk.** `engine::glsl` runs in a few milliseconds and there is nothing to keep in sync; a committed `.frag` is a preview rendering a different shader from the export after one forgotten build step. `tests/renderer/` calls the same function rather than keeping a copy, so its "compute is refused" negative control is about the code that ships.
- **The front end finds uniform offsets by name, from the linked program.** naga owns the GLSL names (`_group_0_binding_0_fs.exposure` today); a table in TypeScript goes stale as one slider that silently does nothing.
- **There is one conversion path from a decoded buffer to the working space.** `to_working_space` takes a described `Surface` — stride, channels, depth — rather than a pointer, because three decoders hand back three shapes. Container features are flattened *before* it (palette, sub-byte depth, `tRNS` and Adam7 by the `png` crate; grey and 16-bit inside the one function), never as a branch in §4's chain. The JPEG path used to expand greyscale itself; that was a second place deciding what grey means, and it is gone.
- **The graph is compiled once, in Rust.** `src/graph/` in the front end is the plan's *executor*. Two compilers would render two topologies and drift the way two shader sources would.
- **Don't add lcms2 to the product.** It is a dev-dependency of `tests/color/` and belongs there: the harness needs to be able to *disagree* with the shipped ICC parser, which it cannot if both call the same library. Same shape as the derived-not-tabulated matrices rule.

**Measurement**

- **Check the instrument first, and a readout is an instrument too.** Twelve times now a confident number has measured the wrong thing, each caught only by asking why a result had the shape it did: a pass sweep of no-ops; a headroom table destroyed by 8-bit quantisation; a GL error hidden behind a green budget table; an agreement test fed different inputs on each side; a ΔE ranking that would have picked the *flattest* gamut policy; a shader disagreement that was the driver's rounding mode; §12.2's 57 codes, which cost two wrong diagnoses before landing on the gamut map; and the PNG round trip's 0.286, which was a display-encoded frame being compared against a linear decode — the number was `to_linear` of the other side, which is what gave it away. The last two were the front end's own frame readout rather than a test: `render`'s duration is CPU *submit* time, because GL commands are asynchronous, so it read 0.00 ms against a 16 ms budget; and the interval between drawn frames is real but `requestAnimationFrame`-bound, so it read 17.0 ms against "budget 16.0 ms" while comfortably meeting it. It reports **fps** now, which is how §7.3 states the requirement and has neither failure mode. And §12.2's orientation check, which scored both parities honestly and then only ever ran the pass count at which both answers agree — the negative control is the proof: turn naga's `ADJUST_COORDINATE_SPACE` off and the two-pass case *still* reads `direct`, because two inversions cancel.
- **A green test can quietly change what it measures.** Freezing the gamut policy took Spike B's test 2 from mean ΔE 0.0807 to 1.4183 against its own 1.5 threshold — still passing, and no longer about transform fidelity, because its reference converter clips and the pipeline no longer did. It is pinned to `GamutPolicy::ClipLinear` now. When a policy constant moves, re-read every test whose reference embeds the old one.
- **§12.2's bar is a precondition, not a number.** Band-limited **and** in-gamut, or the measurement is about an inherent property rather than about the renderer. Under both it is 0.1486 of an 8-bit code; with detail finer than the proxy it is 152. Don't loosen the threshold when it fails — check the fixture still satisfies both conditions.
- **Both harnesses cross-validate on purpose.** `tests/color/` checks ΔE2000 and the matrices against lcms2, and the ICC parser and writer in both directions; `tests/renderer/` keeps a negative control asserting compute is *refused*. Don't delete either as redundant.
- **Generate binary fixtures; do not hand-write them — and check them with something that did not build them.** Both halves have now caught something. Twice a hand-built fixture for a format with an offset table was wrong in a way that looked like a code bug (a JPEG rejected with "invalid length in DHT"; an EXIF block whose `ifd()` forgot the four-byte next-IFD pointer). And Pillow **silently ignored `interlace=True`** — the "interlaced" PNG it wrote was progressive, and only ImageMagick reading it back said so. `tests/fixtures/interlaced.png` is ImageMagick's, verified with Pillow.

**Traps in the code**

- **ICC colorants are in the D50 connection space.** Comparing a tag against a D65 matrix rejects every photograph the app exists to open, so Bradford runs first. And classification is on the whole colorant matrix, not the red one — Adobe RGB is *nearer to Display P3 than sRGB is* (0.0901 against 0.0934), so the obvious classifier reads it as P3, silently.
- **`NodeKind` variants must serialise under an internal tag.** The key builder hashes the serialised node, so a variant serde *cannot* serialise is a panic rather than a compile error. A newtype variant holding a string is that shape — use struct variants.
- **Exports are deterministic and must stay so.** No timestamp in the ICC profile, no encoder state depending on when it ran. §12.1's golden images cannot be blessed otherwise.
- **EXIF orientation is applied to the pixels, never carried forward.** It is structure, not description: a sideways file whose tag says "rotate me" reads correctly only to software honouring the tag, so `metadata: strip` would rotate the photograph. libheif applies the container transform itself; the JPEG path does it explicitly.
- **The photographs are gone from `~/Downloads`.** `real_photos.rs`, `gamut_policy.rs` and `decode_path.rs`'s real-file cases all skip. They are personal files and were never in the repo; put one back or set `PHOTODESK_CORPUS_DIR`. The ΔL\*/ΔC\*/ΔH decomposition on real pixels is the one number `DECISIONS.md` still owes.
- **`tests/renderer/web/generated/` is gitignored and regenerable.** `cargo test -p photodesk-renderer-spike` emits it. The browser harness loads the *generated* GLSL rather than a twin, deliberately.
- **`tauri.conf.json`'s CSP must carry `connect-src ipc:`, and nothing says so if it doesn't.** Tauri v2 answers `invoke` over a `fetch` to its own `ipc:` scheme and falls back to `postMessage` — where a `Vec<u8>` is serialised as a JSON array of numbers — the first time that fetch is refused. `default-src 'self'` refuses it. The fallback is permanent for the session, announced only by a `console.warn` in a webview with no devtools, and correct: the photograph is right and 97 MB of proxy costs 6.8 seconds instead of 0.77. `ipc.ts` reports the array shape for that reason.
- **A Tauri command returning `Response` does not arrive in JS as an `ArrayBuffer`.** It arrives as a byte array, and `new Uint16Array(bytes)` does not reinterpret pairs — it builds an array twice as long holding one byte's *value* per slot. Every f16 becomes a denormal near zero, so the texture is black; `texImage2D` accepts an over-long buffer without complaint, so **nothing raises an error anywhere**. `ipc.ts` reinterprets through `bytesOf` and checks the resulting length against `width × height × 4`. Do not delete that check: it is the only thing standing between this bug and a silent black photograph.
- **A WebGL pass does *not* invert the image, and the obvious reasoning says it does.** GL's framebuffer origin is bottom-left and wgpu's is top-left, so the same `uv` would address the opposite end — except `engine::glsl` lowers with naga's `ADJUST_COORDINATE_SPACE`, which negates `gl_Position.y` and settles it. A pass is identity in index space at any count. `preview.ts` therefore flips **once, at the blit, unconditionally**; the parity rule it used to derive from the pass count was right only for even counts, and an unedited photograph is one pass, so v0.1 presented every photograph upside down until the first slider moved. `glsl.rs` states the writer flag rather than inheriting naga's default, because the blit depends on it.
- **`blitFramebuffer` refuses to copy float → fixed-point** (ES 3.0 §4.3.2), so the plan's output node draws into an 8-bit target — which is right anyway, stage 13 having encoded for the display by then. It raises nothing the user can see: the symptom is a blank canvas.
- **A WebKit `get_snapshot` will not capture a WebGL canvas unless something forced a composite immediately before.** Two "blank canvas" investigations were this and not a bug. The agreement harness is the instrument; a screenshot is not.
- **The front end can put a sentence on stderr** — `ipc.log`, and the `log` command behind it. Every error already lands in a notice, and a notice is invisible when the thing that failed is the preview starting up. Use it; it is how the black canvas was eventually found. Note that `println!` block-buffers when stdout is a pipe, so `log` writes to stderr for both levels.
- **`preview.diagnose()` and its `canvas centre` pixel are kept on purpose.** "A frame was drawn" and "a frame with a photograph in it was drawn" are different claims, and for one long session there was no way to tell them apart from outside the window.
- **`png` swallows an `iCCP` chunk it cannot inflate** and then reports no profile, so a damaged colour claim arrives identical in shape to a file that never made one. §4 says those are different, so `decode.rs` walks the chunk headers itself to tell them apart. Don't replace that with `info.icc_profile.is_none()`.
- **A 4-bit PNG palette packs two pixels to a byte, high nibble first.** A 1×1 fixture holding `0x10` selects index 1 while looking like it selects index 0 — which is how the first version of that test passed while asserting nothing.
- **`libheif-rs` is pinned to 2.7, not 3.x.** 3.x needs libheif ≥ 1.23 and Fedora ships 1.21.2. Load-bearing — the binding tracks upstream closely and Fedora will lag it.

## Open questions

In the register, due before their named milestone:

- **§16 #13** — how the RPM handles HEVC. It cannot require RPM Fusion, so it is `Recommends` plus runtime detection, or nothing. The *decode* half is done: a missing codec is a named error carrying the package name.
- **§16 #14 and #15** — golden-image thresholds, which are one conversation: they must clear both the ~0.9 ΔE YCbCr floor a HEIC arrives with and one colour-attachment step, because RGBA16F truncates rather than rounds on this adapter. Both due before the first `--bless`.
- **§16 #16** — parameter ranges. The document validates finiteness and no bounds, deliberately: §6.1 states none and §11 puts slider travel in the UI. Due before v0.2's presets, the first thing to write params the UI did not.
- **§16 #4** — v1 pipeline ordering, *and the formulations inside it*. The six sliders each have a stated shape in `adjust.wgsl` and several are choices rather than facts. Golden-image work.

From `docs/REVIEW-2026-09-05.md`, still live:

- **§10.1 clipping warnings.** Spike A gave it a data point: RapidRAW paints clipped pixels pure red and blue directly on the photograph, replacing the pixel. Worth looking at before deciding — it is the maximal version of what §10.1 objects to.
- **`providers/` as a Cargo workspace member.** The workspace now has three members including `src-tauri`, so the precedent exists and this is cheap to settle.
