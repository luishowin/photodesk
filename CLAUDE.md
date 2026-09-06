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

**Spec v0.19. Phase 0 complete, v0.1 in progress.** 131 tests, everything on `main` and pushed. (`cargo test` prints two `failed to parse serde attribute` warnings from ts-rs; both are benign — the `ts(type = "string")` override already emits what they would have, and `deny_unknown_fields` has no TypeScript meaning.)

Phase 0's three spikes ran on 2026-09-05 and two of the three answers were not the expected ones — Spike A's gate **failed** (no fork), B and C are green. Their reports are `docs/{FORK-AUDIT,SPIKE-B,SPIKE-C}.md`. Read `FORK-AUDIT.md` before revisiting anything about the render path.

v0.1's headless half is built. What remains is the part a person touches.

| | state |
|---|---|
| Document model (§6.1, §6.3) | done — schema, validation, migration, sidecar IO |
| Graph compile (§6.2) | done — Merkle-keyed DAG, dedup, dirty tracking |
| Decode (§5 stage 0) | done — HEIF + JPEG + PNG → linear P3 f16, ICC classified |
| Render (§7.2) | done — a compiled graph through wgpu, six sliders |
| Export (§6.1's `output`) | done — JPEG + PNG, ICC-tagged, metadata policy |
| **Front end** | **not started** — this is what's next |
| Crop / masks / curves | v0.2–v0.4, refused by name where reachable |

**§12.2 and §12.3 both run.** Source preservation has all five of its steps for the first time; proxy/full-res agreement is measured and its bar is a *precondition* rather than a number (see below).

### What's next

**The front end.** Everything above is headless. §14's v0.1 is a photograph a person can open, drag six sliders on, compare against the original, and export — which means Tauri, the WebGL2 preview executing a compiled plan, and §11's slider contract hand-written (§10.3 froze: no framework, TypeScript + Vite, zero runtime dependencies).

Tauri needs four devel packages this machine does not have, and installing them needs sudo:

```
sudo dnf install webkit2gtk4.1-devel gtk3-devel librsvg2-devel openssl-devel
```

**§16 #17 (PNG) is closed** — decode reads PNG, §1's screenshot is a subject the app can open, and export's `iCCP` finally has a reader. #18 (tiled export) and #19 (HEIF output) can wait; nothing else is open before v0.1 but the front end.

## Where the code is

```
src-tauri/                  the Rust core. A library — no `tauri` dependency yet, and
│                           that is deliberate: §12.1 and §12.3 must run headless, which
│                           is criterion 1 of the gate Spike A failed RapidRAW on
├── engine/                 the render core (§13)
│   ├── colour.rs           spaces, curves, matrices — all derived from chromaticities
│   ├── icc.rs              read and write a profile; §4's two spaces or a named error
│   ├── decode.rs           §5 stage 0: a file becomes linear P3 f16
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
src/document/generated/     TypeScript types, emitted from the Rust schema, committed
tests/color/                Spike B's harness, kept permanent. Tests the product now
tests/renderer/             Spike C's harness, kept permanent
```

`cargo test --workspace` runs everything in a few seconds — `photodesk` is 88 tests, `photodesk-color` 26, `photodesk-renderer-spike` 8. The browser half of Spike C runs on demand: `python3 tests/renderer/web/run-probe.py [--engine webkit2gtk-4.1]`.

## Things that will bite

**Process**

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling.
- **`FORK-AUDIT.md`, `SPIKE-B.md` and `SPIKE-C.md` are spike reports, not living documents.** They record what was measured on 2026-09-05. If something in one turns out wrong, that is a `DECISIONS.md` entry.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page, updated **on request only** — do not regenerate it as a side effect of other work. **It is very stale**: spec v0.4, all three spikes open, the licensing review still gating Spike A. Fourteen spec versions and all of v0.1 behind.

**One source, never two**

This is the project's recurring shape, and it has now decided five things:

- **Never author a compute shader, a storage buffer or a storage texture.** Frozen. naga refuses all three by name targeting GLSL ES 3.00, so one of them anywhere breaks the preview path for the whole project. `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns.
- **There is no CPU renderer and there must not be one.** The obvious way to test a GPU renderer is to write the same maths in Rust and compare — precisely the drift §0 freezes against. The shaders are the only description of what a pixel goes through. The two constants hardcoded in `adjust.wgsl` (linear P3's luma weights and XYZ matrix) are the exception, and a test reads them out of the shader text and compares them against the derived values.
- **`src/document/generated/{document,graph}.ts` are generated and committed.** Don't hand-edit them, and don't hand-write a second TypeScript description of either. `cargo test -p photodesk` regenerates and fails if they changed: change the Rust, run the tests once, commit both.
- **There is one conversion path from a decoded buffer to the working space.** `to_working_space` takes a described `Surface` — stride, channels, depth — rather than a pointer, because three decoders hand back three shapes. Container features are flattened *before* it (palette, sub-byte depth, `tRNS` and Adam7 by the `png` crate; grey and 16-bit inside the one function), never as a branch in §4's chain. The JPEG path used to expand greyscale itself; that was a second place deciding what grey means, and it is gone.
- **The graph is compiled once, in Rust.** `src/graph/` in the front end is the plan's *executor*. Two compilers would render two topologies and drift the way two shader sources would.
- **Don't add lcms2 to the product.** It is a dev-dependency of `tests/color/` and belongs there: the harness needs to be able to *disagree* with the shipped ICC parser, which it cannot if both call the same library. Same shape as the derived-not-tabulated matrices rule.

**Measurement**

- **Check the instrument first.** Eight times now a confident number has measured the wrong thing, each caught only by asking why a result had the shape it did: a pass sweep of no-ops; a headroom table destroyed by 8-bit quantisation; a GL error hidden behind a green budget table; an agreement test fed different inputs on each side; a ΔE ranking that would have picked the *flattest* gamut policy; a shader disagreement that was the driver's rounding mode; §12.2's 57 codes, which cost two wrong diagnoses before landing on the gamut map; and the PNG round trip's 0.286, which was a display-encoded frame being compared against a linear decode — the number was `to_linear` of the other side, which is what gave it away.
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
