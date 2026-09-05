# PhotoDesk — working notes

A display-referred photo editor for Fedora / GNOME. Read `docs/ARCHITECTURE.md` before proposing anything; it is the master spec and it is unusually prescriptive on purpose.

The only code in the tree is Spike B's colour harness at `tests/color/`. There is no product code, and that is deliberate — §14 gives v0.0 none.

## How this project works

The spec's §0 register is the contract. Every item is either **FROZEN** (with a written reason) or **PROVISIONAL** (with a named spike that resolves it). Anything not in the register is undecided, and a decision must be recorded there before code depends on it.

Two rules govern changes to it:

- **Don't freeze without a reason you can write in a sentence.** If it can't be justified, it isn't frozen, it's a habit.
- **Don't mark something provisional without naming its exit condition.** "Provisional" with no exit is indecision with better PR.

When something moves between states, append to `docs/DECISIONS.md` — what moved, what resolved it, the date. The register is the current state; that file is the history. Don't edit past entries.

## Current state

v0.0. Spec is at v0.6. **Spikes A and B are done; C has not been run.**

**Spike A** (2026-09-05) audited RapidRAW 1.6.3 and returned *do not fork*. Read `docs/FORK-AUDIT.md` before revisiting anything about the render path. Short version: the per-pixel chain is one `@compute` kernel in which stage order is the literal statement order, and vendored shaders are read-only, so the frozen "pipeline order is explicit and versioned" was unimplementable in a fork. Separately, RapidRAW has no colour management at all, cannot open HEIF, and on Linux ships every preview frame as a lossy JPEG over IPC.

**Spike B** (2026-09-05) is green. `docs/SPIKE-B.md`, harness at `tests/color/`. The working space is frozen as linear Display P3 f16: it costs ΔE 0.0956 at thirty passes against a budget of 1.0, and the deep-shadow ramp is bit-exact. §2.2's f32 fallback will not be built.

**The next action is Spike C**, the preview renderer (§2.3) — the last one. Node and npm are not installed and it needs them.

## Things that will bite

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling. The specification-only constraint has lifted, but it lifted *because* nothing is being copied — don't undo the premise.
- **Spike C's expected outcome is open, not settled either way.** The compute chain with no GLSL ES 3.0 target was RapidRAW's, and we are not forking it. Ours is authored fragment-first, which stays inside what GLSL has, so naga transpilation is a live branch again. That is a hypothesis. Spike C runs naga against a real fragment shader before anyone claims either result.
- **Spike B never touched a GPU, and freezing f16 on it concentrated a risk rather than retiring one.** The harness is a CPU model. §2.3's `EXT_color_buffer_float` clause — RGBA16F as a colour-renderable target with linear filtering on this machine's WebKitGTK — is still unchecked, and §2.3 says plainly that B and C can each be green and jointly wrong without it. Check it first in Spike C.
- **The colour harness cross-validates itself against lcms2 on purpose.** Two of its eight tests check *the harness* rather than the pipeline. Don't delete them as redundant: an error in ΔE2000 or in the matrix construction would make every threshold in the file meaningless and green.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page. It is updated **on request only** — do not regenerate it as a side effect of other work. **It is currently stale**: it still shows spec v0.4, all three spikes open, and the licensing review gating Spike A.
- **Don't add product code to hit a milestone early.** §14 gives v0.0 no product. Two spikes remain, and skipping them is the specific failure the whole document is arranged to prevent. Spike A is the evidence that the arrangement works — it cost a day and saved a fork.
- **`FORK-AUDIT.md` and `SPIKE-B.md` are spike reports, not living documents.** They record what was measured on 2026-09-05. Don't revise them; if something in one turns out wrong, that is a `DECISIONS.md` entry.

## Open questions worth raising

Listed at the end of `docs/REVIEW-2026-09-05.md`. Status of the three:

- **Phase 0 estimate** — resolved. Moved to 3 weeks, the number the review called honest.
- **§10.1 clipping warnings** — still live, and Spike A gave it a data point: RapidRAW paints clipped pixels pure red and pure blue directly on the photograph, replacing the pixel entirely. Worth looking at before deciding, since it is the maximal version of the thing §10.1 objects to elsewhere.
- **`providers/` as a Cargo workspace member** — the workspace now exists (root `Cargo.toml`, one member: `tests/color`). Still unstated for `providers/`, and now cheap to settle by precedent.

Two new ones opened by Spike B, both in the register: **§4 never named an export gamut-mapping policy** (the harness uses clip-in-linear, which is a choice currently made in a test file), and **ICC extraction from real containers is untested** because two corpus items need `libheif-devel`.
