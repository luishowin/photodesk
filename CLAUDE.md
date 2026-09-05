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

v0.0, **Phase 0 complete**. Spec is at v0.8. All three spikes have run and two of the three answers were not the expected ones.

**Spike A** (2026-09-05) audited RapidRAW 1.6.3 and returned *do not fork*. Read `docs/FORK-AUDIT.md` before revisiting anything about the render path. Short version: the per-pixel chain is one `@compute` kernel in which stage order is the literal statement order, and vendored shaders are read-only, so the frozen "pipeline order is explicit and versioned" was unimplementable in a fork. Separately, RapidRAW has no colour management at all, cannot open HEIF, and on Linux ships every preview frame as a lossy JPEG over IPC.

**Spike B** (2026-09-05) is green. `docs/SPIKE-B.md`, harness at `tests/color/`. The working space is frozen as linear Display P3 f16: it costs ΔE 0.0956 at thirty passes against a budget of 1.0, and the deep-shadow ramp is bit-exact. §2.2's f32 fallback will not be built.

**Spike C** (2026-09-05) is green, and reversed Spike A's apparent verdict on §7.2. `docs/SPIKE-C.md`, harness at `tests/renderer/`. Authored fragment-first, the WGSL lowers to GLSL ES 3.00, WebKitGTK compiles it, six layers cost 5.92 ms at 2 MP against 16 ms, and the WebGL2 and wgpu paths agree to max 0.0088 across 196,608 samples. `EXT_color_buffer_float` is present with RGBA16F colour-renderable and linear-filterable, which retires the risk that B and C could each be green and jointly wrong.

**v0.1 is unblocked.** Three things come first, none of them a spike: name §4's export gamut-mapping policy, finish the two §2.2 corpus items now that `libheif-devel` is installed, and confirm webkit2gtk-4.1 behaves like the webkitgtk-6.0 Spike C measured in.

**The front end has no framework** (2026-09-06, §10.3): TypeScript and Vite, zero runtime dependencies. Don't add React or a component library to make a panel easier — §11's slider contract is the reason the decision went this way, and a library's slider would be overridden rather than used. Panels, undo, focus and the keymap are hand-written by design.

## Things that will bite

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling. The specification-only constraint has lifted, but it lifted *because* nothing is being copied — don't undo the premise.
- **Never author a compute shader, a storage buffer or a storage texture.** This is now a frozen register item, not a preference. naga refuses all three by name when targeting GLSL ES 3.00, so one of them anywhere breaks the preview path for the whole project. `@fragment`, `var<uniform>`, sampled textures, `@location(0)` returns.
- **Both harnesses cross-validate themselves on purpose.** `tests/color/` checks its ΔE2000 and its matrices against lcms2; `tests/renderer/` keeps a negative control that asserts compute is *refused*. Don't delete either as redundant — an error in ΔE2000 would make every colour threshold meaningless and green, and without the control the fragment result is a coincidence rather than a consequence.
- **Three spikes in a row produced a confident number that measured the wrong thing**, each caught only by looking at *why* a result had the shape it did: a pass sweep of no-ops, a headroom table destroyed by 8-bit quantisation, a GL error hidden behind a green budget table, and an agreement test fed different inputs on each side. When a measurement comes back suspiciously clean or suspiciously catastrophic, that is the signal to check the instrument first.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page. It is updated **on request only** — do not regenerate it as a side effect of other work. **It is badly stale**: it still shows spec v0.4, all three spikes open, and the licensing review gating Spike A. Three spec versions and a completed Phase 0 behind.
- **Don't add product code to hit a milestone early.** §14 gives v0.0 no product. Two spikes remain, and skipping them is the specific failure the whole document is arranged to prevent. Spike A is the evidence that the arrangement works — it cost a day and saved a fork.
- **`FORK-AUDIT.md`, `SPIKE-B.md` and `SPIKE-C.md` are spike reports, not living documents.** They record what was measured on 2026-09-05. Don't revise them; if something in one turns out wrong, that is a `DECISIONS.md` entry.
- **`tests/renderer/web/generated/` is gitignored and regenerable.** `cargo test -p photodesk-renderer-spike` emits it. The browser harness loads the *generated* GLSL rather than a hand-written twin, deliberately — a twin would make §0's one-shader-source invariant untestable.

## Open questions worth raising

Listed at the end of `docs/REVIEW-2026-09-05.md`. Status of the three:

- **Phase 0 estimate** — resolved. Moved to 3 weeks, the number the review called honest.
- **§10.1 clipping warnings** — still live, and Spike A gave it a data point: RapidRAW paints clipped pixels pure red and pure blue directly on the photograph, replacing the pixel entirely. Worth looking at before deciding, since it is the maximal version of the thing §10.1 objects to elsewhere.
- **`providers/` as a Cargo workspace member** — the workspace now exists (root `Cargo.toml`, members `tests/color` and `tests/renderer`). Still unstated for `providers/`, and now cheap to settle by precedent.

Two live in the register: **§4's export gamut-mapping policy** (the harness uses clip-in-linear, a choice currently made in a test file), and **ICC extraction from real containers** (`libheif-devel` is now installed, so the two blocked §2.2 corpus items are buildable).
