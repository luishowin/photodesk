/**
 * The Light tab — §14's v0.1, which is six controls and no more.
 *
 * > **0.1** Open iPhone HEIF → exposure, contrast, highlights, shadows, blacks,
 * > temperature → before/after → export → colour correct end to end.
 *
 * `adjust.wgsl` also carries tint, vibrance and saturation, because §5 puts them in
 * stages 2 and 9 and the document has always had them. They are **not** here: §14 puts
 * the Colour tab at v0.3, and "ship v0.1 before designing v0.4" is the sentence the
 * roadmap ends on. A control that exists because the shader happens to have a uniform
 * for it is scope arriving through the back door.
 *
 * **This file is where §16 #16 currently lives.** The register: "parameter ranges — the
 * document validates finiteness but no bounds… §11 puts slider travel in the UI;
 * whether the *file* has an opinion is unstated". So these numbers are travel, not
 * validation: a document may legally carry `exposure: 400` and this build will render
 * it, it simply cannot be dragged there. The decision is due before v0.2's presets,
 * which will be the first thing writing parameters no slider produced.
 */

import { type SliderHandlers, type SliderSpec, Slider } from "./slider";

/** §5's stage-2 reference — the working space's own white point, 6504 K. */
const REFERENCE_KELVIN = 6504;

export const LIGHT: SliderSpec[] = [
  {
    key: "exposure",
    label: "Exposure",
    // Stops: the shader is `exp2(exposure)`. ±3 is four doublings either way, which is
    // past anything a finished image survives and short of a range that makes the
    // useful part of the track a few pixels wide.
    min: -3,
    max: 3,
    default: 0,
    step: 0.05,
    unit: "EV",
    places: 2,
  },
  {
    key: "contrast",
    label: "Contrast",
    min: -100,
    max: 100,
    default: 0,
    step: 1,
    places: 0,
  },
  {
    key: "highlights",
    label: "Highlights",
    min: -100,
    max: 100,
    default: 0,
    step: 1,
    places: 0,
  },
  { key: "shadows", label: "Shadows", min: -100, max: 100, default: 0, step: 1, places: 0 },
  { key: "blacks", label: "Blacks", min: -100, max: 100, default: 0, step: 1, places: 0 },
  {
    key: "temperature",
    label: "Temperature",
    // Stored as a delta from 6504 K and shown as an absolute figure (§6.1). The travel
    // is chosen so the *displayed* ends are round numbers — 4000 K to 10000 K — because
    // those are the ones a photographer reads, and because 4000 K is exactly where
    // `daylight_xy` stops being defined and starts clamping.
    min: 4000 - REFERENCE_KELVIN,
    max: 10000 - REFERENCE_KELVIN,
    default: 0,
    step: 50,
    unit: "K",
    places: 0,
    displayOffset: REFERENCE_KELVIN,
    signed: false,
  },
];

/** The tabs §10.3 draws, and which version each one arrives in. */
export const TABS: { name: string; version: string | null }[] = [
  { name: "Crop", version: "0.2" },
  { name: "Light", version: null },
  { name: "Color", version: "0.3" },
  { name: "Detail", version: "0.3" },
  { name: "Effects", version: "0.3" },
  { name: "Masks", version: "0.4" },
  { name: "AI", version: "0.5" },
];

export function buildLightPanel(handlers: SliderHandlers): {
  element: HTMLElement;
  sliders: Map<string, Slider>;
} {
  const panel = document.createElement("div");
  panel.className = "panel";
  const sliders = new Map<string, Slider>();
  for (const spec of LIGHT) {
    const slider = new Slider(spec, handlers);
    sliders.set(spec.key, slider);
    panel.append(slider.element);
  }
  return { element: panel, sliders };
}
