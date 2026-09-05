# PhotoDesk — working notes

A display-referred photo editor for Fedora / GNOME. **There is no code yet, and that is deliberate.** Read `docs/ARCHITECTURE.md` before proposing anything; it is the master spec and it is unusually prescriptive on purpose.

## How this project works

The spec's §0 register is the contract. Every item is either **FROZEN** (with a written reason) or **PROVISIONAL** (with a named spike that resolves it). Anything not in the register is undecided, and a decision must be recorded there before code depends on it.

Two rules govern changes to it:

- **Don't freeze without a reason you can write in a sentence.** If it can't be justified, it isn't frozen, it's a habit.
- **Don't mark something provisional without naming its exit condition.** "Provisional" with no exit is indecision with better PR.

When something moves between states, append to `docs/DECISIONS.md` — what moved, what resolved it, the date. The register is the current state; that file is the history. Don't edit past entries.

## Current state

v0.0, pre-code. Spec is at v0.4. **None of the three Phase 0 spikes has been run**, so nothing in the register has been frozen by evidence yet.

The next action is Spike A, the RapidRAW fork audit. Source is unpacked at `~/Downloads/RapidRAW-main` (v1.6.3, AGPL-3.0). Part of the audit is already recorded in §2.1 — start from that table.

## Things that will bite

- **The repository is specification-only until Spike A concludes**, and it is public. RapidRAW is AGPL-3.0 and the licensing review is sequenced to gate that decision. Don't vendor RapidRAW code into this repository before that gate.
- **RapidRAW's shader chain is compute end to end** — `@compute` entry points, a `texture_storage_2d` output, a storage buffer of adjustments. GLSL ES 3.0 has no compute shaders, no storage textures and no storage buffers, so §7.2's naga transpilation has nothing to lower onto. Spike C is framed around the hand-written fallback being the expected outcome.
- **`docs/` is the GitHub Pages source.** `docs/index.html` is the published status page. It is updated **on request only** — do not regenerate it as a side effect of other work.
- **Don't add product code to hit a milestone early.** §14 gives v0.0 no product. The spikes exist because their answers are expensive to discover late, and skipping them is the specific failure the whole document is arranged to prevent.

## Open questions worth raising

Listed at the end of `docs/REVIEW-2026-09-05.md`. The live ones: the Phase 0 estimate is optimistic; §10.1 permits colour for clipping warnings drawn directly on the photograph, which is the same objection it raises against a red mask overlay; and whether `providers/` is a Cargo workspace member is unstated.
