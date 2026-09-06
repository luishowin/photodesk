/**
 * Changing a document, and the one structural rule v0.1 needs.
 *
 * §6.1 via `Document::new`: the stack starts **empty rather than seeded with a global
 * layer** — "§5's loop body skips identity stages entirely, so a seeded all-identity
 * layer would be a row in the UI, a line in every diff and a node the graph has to
 * eliminate, in exchange for nothing. The global layer is created when the first slider
 * moves." This is where it gets created.
 *
 * Every function here returns a new document rather than mutating one. That is not a
 * style preference: history holds documents (see `history.ts`), and a mutated document
 * would rewrite the entries already in it.
 */

import type { AdjustV1, Document, Layer } from "./generated/document";

/** §6.1's own name for the layer that has no mask. `global` is what the tests call it. */
export const GLOBAL_LAYER = "global";

/** The adjustments in force, or an empty set — which §6.1 says means identity. */
export function adjustments(document: Document): AdjustV1 {
  const layer = document.stack.find((l) => l.id === GLOBAL_LAYER && l.enabled);
  return layer ? (layer.params as AdjustV1) : {};
}

/**
 * Set one parameter, creating the global layer if this is the first move.
 *
 * A parameter set back to its default is **removed** rather than written as zero.
 * §6.1's omitted-key rule makes those two different documents with the same appearance,
 * and the difference matters the moment a preset merges into the stack: a preset that
 * says nothing about exposure has to leave exposure alone, so "exposure: 0" is a claim
 * and an absent key is silence. Writing zeros would turn every document into a preset
 * that overrides everything.
 */
export function setParameter(document: Document, key: string, value: number): Document {
  const next: Document = { ...document, stack: [...document.stack] };
  const at = next.stack.findIndex((l) => l.id === GLOBAL_LAYER);

  const params: Record<string, number | null | undefined> =
    at >= 0 ? { ...(next.stack[at] as Layer).params } : {};
  if (value === 0) delete params[key];
  else params[key] = value;

  if (Object.keys(params).length === 0) {
    // The last adjustment came off, so the layer goes with it — back to the document
    // `Document::new` would have produced, byte for byte.
    if (at >= 0) next.stack.splice(at, 1);
    return next;
  }

  const layer: Layer = {
    id: GLOBAL_LAYER,
    op: "adjust",
    op_version: 1,
    enabled: true,
    params: params as AdjustV1,
    ...(at >= 0 ? { name: (next.stack[at] as Layer).name } : {}),
  };
  if (at >= 0) next.stack[at] = layer;
  else next.stack.push(layer);
  return next;
}

/**
 * The same document with nothing applied — §11's "show original".
 *
 * The stack is emptied and everything else kept, so the comparison runs through the
 * same `output` block and the same pipeline version. An "original" that also changed
 * the output space would be comparing two different questions.
 */
export function withoutEdits(document: Document): Document {
  return { ...document, stack: [] };
}

/** Whether anything has been done to this photograph. Drives the before/after affordance. */
export function isEdited(document: Document): boolean {
  return document.stack.length > 0;
}
