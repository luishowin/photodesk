/**
 * §11's undo, at gesture granularity.
 *
 * Two register items shape this. **"History is session-only, never serialised"** — a
 * sidecar accumulating every gesture grows without bound (§6.1) — so this holds nothing
 * that outlives the window. And §11's "one completed gesture is one undo entry": a
 * slider drag is one record, not two hundred, which is why the slider has a separate
 * `onCommit` and why nothing here is called during a drag.
 *
 * §7.3 budgets "undo depth ≥ 100 gestures. **Deltas, not pixels** — cheap." These are
 * whole documents rather than deltas, and at v0.1 that is the same thing: a document
 * with one layer of nine optional numbers is a few hundred bytes, so a hundred of them
 * is smaller than one row of the proxy. When the stack grows masks and curves (v0.4,
 * v0.3) this is the file to revisit, and the shape above — commit whole states at
 * gesture boundaries — is what a delta would replace, not the granularity.
 */

import type { Document } from "./generated/document";

const DEPTH = 200;

export class History {
  private past: Document[] = [];
  private future: Document[] = [];
  private present: Document;

  constructor(initial: Document) {
    this.present = initial;
  }

  get current(): Document {
    return this.present;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  /** Start again from a freshly opened photograph. Nothing carries across documents. */
  reset(document: Document): void {
    this.past = [];
    this.future = [];
    this.present = document;
  }

  /** One completed gesture. A commit that changes nothing is not an entry. */
  commit(document: Document): void {
    if (JSON.stringify(document) === JSON.stringify(this.present)) return;
    this.past.push(this.present);
    if (this.past.length > DEPTH) this.past.shift();
    this.future = [];
    this.present = document;
  }

  /**
   * The live document during a gesture — rendered, never recorded.
   *
   * This is what keeps a drag out of the history: the preview follows `present` and
   * `present` moves continuously, but `past` only grows in `commit`.
   */
  preview(document: Document): void {
    this.present = document;
  }

  undo(): Document | null {
    const previous = this.past.pop();
    if (previous === undefined) return null;
    this.future.push(this.present);
    this.present = previous;
    return this.present;
  }

  redo(): Document | null {
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(this.present);
    this.present = next;
    return this.present;
  }
}
