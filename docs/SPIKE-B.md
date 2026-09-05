# Spike B — colour validation harness

**Date:** 2026-09-05
**Subject:** §4's candidate working space — linear Display P3, f16
**Harness:** `tests/color/`, 8 tests, all green
**Deliverable for:** `ARCHITECTURE.md` §2.2

---

## Verdict

**Freeze the working space: linear Display P3, f16.**

At the deepest chain §7.3 permits — thirty render passes, six layers' worth — f16 storage costs **ΔE2000 0.0956 max, 0.0323 mean** against the identical chain computed in f64. §2.2's tightest threshold and §12.1's golden-image budget are both ΔE 1.0, so **f16 consumes under a tenth of the correctness budget** at the worst depth the architecture allows.

§2.2's stated fallback — f32 working buffers at proxy resolution — is not needed and should not be built.

**One thing is not signed off, and it is not the working space.** Two of the six §2.2 corpus items need `libheif-devel`, which is not installed. Both test ICC extraction from a real container rather than the working space itself, so they gate a different question — see [What is not proven](#what-is-not-proven).

---

## Results

All figures are ΔE2000. `n` is the number of colour samples in the corpus.

### The four §2.2 tests

| Test | Threshold | Result | Margin |
|---|---|---|---|
| 1 — round-trip identity, zero adjustments | max < 1.0 | **max 0.0000** (n=153) | exact at 8 bits |
| 2 — Display P3 → sRGB vs reference converter | mean < 1.5 | **mean 0.0807**, max 0.4828 (n=126) | 18× |
| 3 — untagged input assumed sRGB | max < 1.0 | **max 0.0000** (n=27) | exact at 8 bits |
| 4 — deep-shadow ramp through the full chain | no banding, max < 2.0 | **all three banding assertions pass, max 0.0000** | exact at 8 bits |

Three of the four read exactly zero because the error is below half an 8-bit code, so the output file is byte-identical to the input. That is the right answer for thresholds stated on a delivered image, and it is useless for asking how much margin the format has — so each is also reported before quantisation: test 1 is **max 0.0172**, test 4 is **max 0.0006**.

### Test 4 in full — the one the spike exists for

```
in  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
out [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]

distinct output codes: 17 of 17          (§2.2 requires >= 14)
first differences:  [1 × 16]
second differences: [0 × 15]             (§2.2 requires max |Δ²| <= 1)
monotonic non-decreasing: yes
```

A bit-exact identity ramp at thirty passes. §2.2 predicted "for 8-bit-sourced material it is very probably fine"; it is fine, and the reason is worth writing down because it generalises. Code 1/255 decodes to linear 3.03 × 10⁻⁴. Near that magnitude an f16 ulp is roughly 2.4 × 10⁻⁷, so there are about **1,270 representable f16 values between adjacent 8-bit codes** at the very bottom of the range. Linear encoding does spend its precision in the highlights, as §2.2 says — but half float's 11-bit significand is *relative*, and 8-bit source material never gets close to exhausting it.

### f16 cost, measured against an f64 reference

Divergence from the identical chain run entirely in f64. Both paths share the same transforms and the same per-pixel workload; only one of them rounds to a 16-bit grid between passes, so the difference is the cost of the format alone.

| Corpus | 1 pass | 5 passes | 30 passes |
|---|---|---|---|
| deep-shadow ramp | 0.0010 | 0.0012 | 0.0002 |
| ColorChecker | 0.0227 | 0.0266 | **0.0956** |
| wide-gamut sweep | 0.0117 | 0.0199 | 0.0596 |

f32 buffers read 0.0000 everywhere, as expected.

Growth is roughly √n, which is what an unbiased random walk gives: ColorChecker rises 4.2× over 30 passes against √30 ≈ 5.5. **Error accumulates as a walk, not as a ratchet** — the shadow ramp is actually *lower* at 30 passes than at 5, because the alternating gain in the workload walks small values back onto exact f16 grid points. A longer chain is not reliably worse, which is the reassuring version of this result and also the reason not to extrapolate it confidently past the measured range.

---

## How the harness avoids grading its own homework

A colour harness that only agrees with itself proves nothing: an error in ΔE2000 or in the matrix construction would make every threshold above meaningless *and* green. Two links, both tested against lcms2 2.16:

| Link | Check | Result |
|---|---|---|
| ΔE2000 | 20,000 random Lab pairs vs `cmsCIE2000DeltaE` | worst absolute difference **1.4 × 10⁻¹³** |
| Matrix construction | 24 ColorChecker patches, sRGB → Display P3, vs an lcms2 `Transform` at relative colorimetric | **max ΔE 0.0000** |

Three further decisions kept the measurement honest, each of which was wrong in a first draft of this harness and is recorded so it does not get re-introduced:

- **No tabulated matrices.** Every RGB↔XYZ matrix is derived from chromaticities at construction time. Constants copied from a website are the classic source of a silent half-percent error, and this is the harness that would have to catch it.
- **One workload definition, not two.** The per-pixel work is written once, generically over a `Real` trait, and instantiated at f32 and f64. Two copies would be free to drift, and a drifting reference measures nothing.
- **The corpus stores sRGB only.** The Display P3 values are derived by the reference converter rather than tabulated, so a second table cannot disagree with the first.

**Test 3 carries a counterexample**, because "assume sRGB" passing at ΔE 0.0000 says nothing unless getting it wrong is detectable. Deliberately misreading the same data as Display P3 gives max ΔE 4.52 on saturated content — above the ΔE 1.0 the test applies, so the test can tell right from wrong.

That counterexample also produced the most interesting number in the spike. On the flat, largely desaturated content a **screenshot** actually contains, misreading the profile costs only **max 2.96, mean 0.50**. Profile misinterpretation is a *quiet* failure on exactly the material most likely to arrive untagged. It is not a thing anybody notices by looking; it is a thing you find with a test or not at all. Spike A found that RapidRAW does this to every P3 file it opens.

---

## Two corrections made during the spike

Recorded because both were green-looking measurements of nothing, and both would have shipped a false statement into `DECISIONS.md`.

**The pass sweep measured no-ops.** The first version ran `passes` *identity* stages and reported a flat line at 1, 5 and 30. Arithmetically correct and completely uninformative: `f16 → f32 → f16` is idempotent, so an identity pass is a no-op and thirty of them are thirty no-ops. It would have supported the sentence "f16 survives a thirty-pass chain" on no evidence whatsoever. Fixed by giving each pass real per-pixel work of the shape §5 stages 2–9 have — a gain, a tonal S-curve, a channel mix — and measuring divergence from the same workload in f64. The control is still in the suite: it prints the flat line and says why it is flat.

**The headroom table measured after 8-bit quantisation.** Every cell read 0.0000 for both f16 and f32, because the quantiser destroys exactly the quantity being reported. Fixed by measuring in the encoded float domain. The §2.2 tests still measure at 8 bits, because their thresholds are stated on the delivered image.

---

## What is not proven

**Two of six corpus items are blocked on `libheif-devel`**, which is absent (the runtime `libheif.so.1.21.2` is present; there is no pkg-config `.pc`). They are declared in `corpus::BLOCKED` and printed by the suite rather than quietly omitted.

| Item | What it would test |
|---|---|
| iPhone HEIF with an embedded Display P3 profile | ICC extraction from a real container. Synthetic patches prove the matrices; they cannot prove we read the profile that says *which* matrices to use. |
| iPhone HEIC carrying an ISO HDR gain map | That the SDR base decodes correctly and the gain map is **ignored rather than misapplied** — a different assertion from "we do not support it" (§4). |

**This does not block freezing the working space, and the distinction matters.** The question §2.2 exists to answer is whether linear P3 at f16 holds up numerically, and that is answered by exact-known-value synthetic corpora — which is what synthetic corpora are *for*. What the HEIF items test is the decode path: whether we correctly read the tag that selects a transform. That is a different register item, and it did not previously exist. It does now (see below), rather than being left as an unnamed gap.

**Three further limits, stated so they are not discovered as surprises:**

- **The workload is representative in shape, not a worst case in magnitude.** Gains of ±0.05 EV and an 8% contrast blend, not +3 EV and a hard curve. f16's relative precision is constant, so magnitude should not change the story — but "should not" is the word this document exists to avoid, and the honest scope of the 0.0956 figure is first-order.
- **f16 buffers, f32 arithmetic.** That is what a GPU with RGBA16F textures actually does, and it is what makes the format self-correcting between passes. A pipeline that computed *in* f16 would be a different measurement.
- **Nothing here has touched a GPU.** These are CPU models of the transforms. Spike C's `EXT_color_buffer_float` clause is what establishes that RGBA16F is a colour-renderable target with linear filtering on this machine — and §2.3 already warns that Spike B and Spike C can each be green and jointly wrong if that clause goes unchecked. It remains unchecked.

---

## One decision §4 does not make

**§4 never names a gamut-mapping policy.** It says "linear P3 → tone encode → sRGB (default) or Display P3 → ICC-tagged file" and stops. A Display P3 source exported to sRGB produces negative channels for everything outside the smaller gamut, and something has to decide what happens to them. Test 2 cannot be written without an answer.

The harness uses **clip each channel to [0,1] in linear light**, recorded as `GamutPolicy::ClipLinear` so the choice is visible rather than emergent. It agrees with lcms2 at relative colorimetric to mean ΔE 0.0807, which is well inside test 2's threshold — so the choice is defensible, but it *is* a choice, and it is currently made in a test harness rather than in the specification.

Left as an open decision rather than resolved here: clipping is the simplest policy and the most predictable, and it is also the one that flattens detail in saturated highlights, which is the visible complaint people have about naive gamut handling. It belongs in §4 before v0.1 exports anything.

---

## Register consequences

| Item | Was | Now |
|---|---|---|
| **Working space = linear Display P3 f16** | PROVISIONAL → Spike B | **FROZEN.** Costs under a tenth of the ΔE 1.0 budget at the deepest chain §7.3 permits. |
| §2.2's f32 fallback | contingency | **Not needed.** Do not build it. |
| **ICC extraction from real containers** | *did not exist* | **PROVISIONAL** → the two blocked corpus items, once `libheif-devel` is installed. |
| **Export gamut-mapping policy** | *unstated in §4* | **PROVISIONAL** → before v0.1 exports. Harness uses clip-in-linear. |

The harness stays as a permanent suite per §13. It runs in about 20 ms and needs no fixtures, so there is no reason it should not be on every commit from here.

---

## Running it

```
cargo test -p photodesk-color -- --nocapture
```

The numbers are the deliverable; the assertions only say they were in range. `--nocapture` is not optional if you want to know anything.
