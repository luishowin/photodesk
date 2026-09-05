# PhotoDesk — working notes

A display-referred photo editor for Fedora / GNOME. Read `docs/ARCHITECTURE.md` before proposing anything; it is the master spec and it is unusually prescriptive on purpose.

The only code in the tree is Phase 0's two harnesses, `tests/color/` and `tests/renderer/`. There is no product code, and that is deliberate — §14 gives v0.0 none.

## How this project works

The spec's §0 register is the contract. Every item is either **FROZEN** (with a written reason) or **PROVISIONAL** (with a named spike that resolves it). Anything not in the register is undecided, and a decision must be recorded there before code depends on it.

Two rules govern changes to it:

- **Don't freeze without a reason you can write in a sentence.** If it can't be justified, it isn't frozen, it's a habit.
- **Don't mark something provisional without naming its exit condition.** "Provisional" with no exit is indecision with better PR.

When something moves between states, append to `docs/DECISIONS.md` — what moved, what resolved it, the date. The register is the current state; that file is the history. Don't edit past entries.

## Current state

v0.0, **Phase 0 complete**. Spec is at v0.12. All three spikes have run and two of the three answers were not the expected ones.

**Spike A** (2026-09-05) audited RapidRAW 1.6.3 and returned *do not fork*. Read `docs/FORK-AUDIT.md` before revisiting anything about the render path. Short version: the per-pixel chain is one `@compute` kernel in which stage order is the literal statement order, and vendored shaders are read-only, so the frozen "pipeline order is explicit and versioned" was unimplementable in a fork. Separately, RapidRAW has no colour management at all, cannot open HEIF, and on Linux ships every preview frame as a lossy JPEG over IPC.

**Spike B** (2026-09-05) is green. `docs/SPIKE-B.md`, harness at `tests/color/`. The working space is frozen as linear Display P3 f16: it costs ΔE 0.0956 at thirty passes against a budget of 1.0, and the deep-shadow ramp is bit-exact. §2.2's f32 fallback will not be built.

**Spike C** (2026-09-05) is green, and reversed Spike A's apparent verdict on §7.2. `docs/SPIKE-C.md`, harness at `tests/renderer/`. Authored fragment-first, the WGSL lowers to GLSL ES 3.00, WebKitGTK compiles it, six layers cost 5.92 ms at 2 MP against 16 ms, and the WebGL2 and wgpu paths agree to max 0.0088 across 196,608 samples. `EXT_color_buffer_float` is present with RGBA16F colour-renderable and linear-filterable, which retires the risk that B and C could each be green and jointly wrong.

**v0.1 is unblocked.** The iPhone HEIC path is tested end to end — `libheif-freeworld` is installed and `heif_codecs.rs` now *asserts* HEVC, so a mis-provisioned machine says so rather than failing to open a photograph. Fedora ships libheif without HEVC on patent grounds; that lands on §13 as §16 #13, because a Fedora RPM may not require a third-party repo, and v0.1's decode path should return a distinguishable "no codec" error rather than a generic failure.

**A HEIC costs ~0.9 ΔE before we see it** — RGB↔YCbCr conversion, identical across libaom and x265 at lossless, so it is inherent and unavoidable on read. §12.1's HEIC thresholds must clear it (§16 #14).

**§4 is verified against real photographs.** An iPhone HEIC and JPEG both tag Display P3 with the same 536-byte profile, the base image is 8-bit, and a real photograph round-trips through linear P3 f16 at max ΔE 0.0000. The gain map is a half-resolution auxiliary image and libheif's default decode ignores it, so v1's discard is a visible skip. **The photographs are not in the repo and must not be** — `real_photos.rs` reads `PHOTODESK_CORPUS_DIR` (default `~/Downloads`) and skips when empty, and reports EXIF by type and size only, never by value.

**§16 #11 is closed: the export gamut policy is `clip chroma at constant luminance`** (2026-09-06, §4, `tests/color/tests/gamut_policy.rs`). It slides a colour along the ray from the achromatic point of its own luminance to the gamut boundary, so L\* is exact and chroma is the only thing spent. Chosen over the clip because *the clip's error has no policy* — how much lightness is lost depends on which channel ran out first, up to 2.8 L\* — and because it halves the adjacent pixel pairs a real photograph merges into one colour, at zero cost inside the gamut. The soft-knee variant is better at gradients and was rejected: it moves 3.2% of an already-correct frame on the strength of a tuning constant with no derivation. It is implemented and swept (`GamutPolicy::CompressLuma`) so reopening it is one line.

Two things it dragged in. **Stage 13 now has a shader** — `tests/renderer/shaders/encode.wgsl`, checked to lower to GLSL ES 3.00 and to agree with the Rust reference to one colour-attachment step. And **RGBA16F attachments can truncate rather than round** (measured on RADV/RENOIR), which is a full-step bias that §12.1's thresholds have to allow for — §16 #15.

**One thing still comes first, and it is not a spike:** confirm webkit2gtk-4.1 behaves like the webkitgtk-6.0 Spike C measured in.

**The front end has no framework** (2026-09-06, §10.3): TypeScript and Vite, zero runtime dependencies. Don't add React or a component library to make a panel easier — §11's slider contract is the reason the decision went this way, and a library's slider would be overridden rather than used. Panels, undo, focus and the keymap are hand-written by design.

**Where the work is.** All of Phase 0 sits on the branch **`spike-a-fork-audit`**, nine commits, nothing pushed — `main` is still at the pre-code commit and tracks `origin/main`. The branch name is left over from when it held only Spike A and is now misleading: it carries all three spikes, the front-end decision, the HEVC findings, the real-photo verification and the export gamut policy. Rename it or merge to `main` before pushing; don't assume `main` reflects any of this.

## Things that will bite

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling. The specification-only constraint has lifted, but it lifted *because* nothing is being copied — don't undo the premise.
- **Never author a compute shader, a storage buffer or a storage texture.** This is now a frozen register item, not a preference. naga refuses all three by name when targeting GLSL ES 3.00, so one of them anywhere breaks the preview path for the whole project. `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns.
- **Both harnesses cross-validate themselves on purpose.** `tests/color/` checks its ΔE2000 and its matrices against lcms2; `tests/renderer/` keeps a negative control that asserts compute is *refused*. Don't delete either as redundant — an error in ΔE2000 would make every colour threshold meaningless and green, and without the control the fragment result is a coincidence rather than a consequence.
- **A green test can quietly change what it measures.** Freezing the gamut policy took Spike B's test 2 from mean ΔE 0.0807 to 1.4183 against its own 1.5 threshold — still passing, and no longer about transform fidelity at all, because its reference converter clips and the pipeline no longer did. It is pinned to `GamutPolicy::ClipLinear` now with the reason next to it. When a policy constant moves, re-read every test whose reference embeds the old one.
- **The photographs are gone from `~/Downloads`** as of this session, so `real_photos.rs` and `gamut_policy.rs`'s real-photograph case both skip. They are personal files and were never in the repo; put one back, or set `PHOTODESK_CORPUS_DIR`, and both run again. The ΔL\*/ΔC\*/ΔH decomposition on real pixels is the one number `DECISIONS.md` still owes.
- **Three spikes in a row produced a confident number that measured the wrong thing**, each caught only by looking at *why* a result had the shape it did: a pass sweep of no-ops, a headroom table destroyed by 8-bit quantisation, a GL error hidden behind a green budget table, and an agreement test fed different inputs on each side. §16 #11 added two more — a ΔE ranking that would have picked the flattest policy, and a shader disagreement that turned out to be the driver's rounding mode. When a measurement comes back suspiciously clean or suspiciously catastrophic, that is the signal to check the instrument first.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page. It is updated **on request only** — do not regenerate it as a side effect of other work. **It is badly stale**: it still shows spec v0.4, all three spikes open, and the licensing review gating Spike A. Three spec versions and a completed Phase 0 behind.
- **Don't add product code to hit a milestone early.** §14 gives v0.0 no product. Two spikes remain, and skipping them is the specific failure the whole document is arranged to prevent. Spike A is the evidence that the arrangement works — it cost a day and saved a fork.
- **`FORK-AUDIT.md`, `SPIKE-B.md` and `SPIKE-C.md` are spike reports, not living documents.** They record what was measured on 2026-09-05. Don't revise them; if something in one turns out wrong, that is a `DECISIONS.md` entry.
- **`tests/renderer/web/generated/` is gitignored and regenerable.** `cargo test -p photodesk-renderer-spike` emits it. The browser harness loads the *generated* GLSL rather than a hand-written twin, deliberately — a twin would make §0's one-shader-source invariant untestable.

## Open questions worth raising

Listed at the end of `docs/REVIEW-2026-09-05.md`. Status of the three:

- **Phase 0 estimate** — resolved. Moved to 3 weeks, the number the review called honest.
- **§10.1 clipping warnings** — still live, and Spike A gave it a data point: RapidRAW paints clipped pixels pure red and pure blue directly on the photograph, replacing the pixel entirely. Worth looking at before deciding, since it is the maximal version of the thing §10.1 objects to elsewhere.
- **`providers/` as a Cargo workspace member** — the workspace now exists (root `Cargo.toml`, members `tests/color` and `tests/renderer`). Still unstated for `providers/`, and now cheap to settle by precedent.

One lives in the register: **how the RPM handles HEVC** (§16 #13 — it cannot require RPM Fusion, so it is `Recommends` plus runtime detection or nothing). §16 #14 and #15 are the same conversation with each other: golden-image thresholds have to clear both the ~0.9 ΔE YCbCr floor and one colour-attachment step, and both are due before the first `--bless`.

`libheif-rs` is pinned to **2.7**, not 3.x: 3.x requires libheif ≥ 1.23 and Fedora ships 1.21.2. That pin is load-bearing — the binding tracks upstream closely and Fedora will lag it.
