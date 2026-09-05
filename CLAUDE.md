# PhotoDesk — working notes

A display-referred photo editor for Fedora / GNOME. **There is no code yet, and that is deliberate.** Read `docs/ARCHITECTURE.md` before proposing anything; it is the master spec and it is unusually prescriptive on purpose.

## How this project works

The spec's §0 register is the contract. Every item is either **FROZEN** (with a written reason) or **PROVISIONAL** (with a named spike that resolves it). Anything not in the register is undecided, and a decision must be recorded there before code depends on it.

Two rules govern changes to it:

- **Don't freeze without a reason you can write in a sentence.** If it can't be justified, it isn't frozen, it's a habit.
- **Don't mark something provisional without naming its exit condition.** "Provisional" with no exit is indecision with better PR.

When something moves between states, append to `docs/DECISIONS.md` — what moved, what resolved it, the date. The register is the current state; that file is the history. Don't edit past entries.

## Current state

v0.0, pre-code. Spec is at v0.5. **Spike A is done and failed its gate; B and C have not been run.**

Spike A (2026-09-05) audited RapidRAW 1.6.3 and returned *do not fork*. Read `docs/FORK-AUDIT.md` before revisiting anything about the render path — it is the only document in the tree backed by measurement rather than argument. The short version: RapidRAW's per-pixel chain is one `@compute` kernel in which stage order is the literal statement order, and vendored shaders are read-only, so the frozen "pipeline order is explicit and versioned" was unimplementable in a fork. Separately, RapidRAW has no colour management at all, cannot open HEIF, and on Linux ships every preview frame as a lossy JPEG over IPC.

**The next action is Spike B**, the colour validation harness (§2.2). It is now the critical path: §4 has no incumbent to adapt, so it gets built from nothing and the harness decides whether it is built on f16. First command is `sudo dnf install libheif-devel`.

## Things that will bite

- **Never vendor RapidRAW code.** The gate failed, so there is no fork and no reason to. It is AGPL-3.0 and this repository is public. `FORK-AUDIT.md` quotes identifiers and line numbers for audit purposes; that is the ceiling. The specification-only constraint has lifted, but it lifted *because* nothing is being copied — don't undo the premise.
- **Spike C's expected outcome is open, not settled either way.** The compute chain with no GLSL ES 3.0 target was RapidRAW's, and we are not forking it. Ours is authored fragment-first, which stays inside what GLSL has, so naga transpilation is a live branch again. That is a hypothesis. Spike C runs naga against a real fragment shader before anyone claims either result.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page. It is updated **on request only** — do not regenerate it as a side effect of other work. **It is currently stale**: it still shows spec v0.4, all three spikes open, and the licensing review gating Spike A.
- **Don't add product code to hit a milestone early.** §14 gives v0.0 no product. Two spikes remain, and skipping them is the specific failure the whole document is arranged to prevent. Spike A is the evidence that the arrangement works — it cost a day and saved a fork.
- **`FORK-AUDIT.md` is a spike report, not a living document.** It records what was true of RapidRAW 1.6.3 on 2026-09-05. Don't revise it; if something in it turns out wrong, that is a `DECISIONS.md` entry.

## Open questions worth raising

Listed at the end of `docs/REVIEW-2026-09-05.md`. Status of the three:

- **Phase 0 estimate** — resolved. Moved to 3 weeks, the number the review called honest.
- **§10.1 clipping warnings** — still live, and Spike A gave it a data point: RapidRAW paints clipped pixels pure red and pure blue directly on the photograph, replacing the pixel entirely. Worth looking at before deciding, since it is the maximal version of the thing §10.1 objects to elsewhere.
- **`providers/` as a Cargo workspace member** — still unstated, and more pressing now that the tree will be real. Decide it when the workspace is created rather than after.
