/**
 * §11's slider, which is why there is no component library.
 *
 * §10.3: "§11 specifies the interaction surface down to `Shift`-drag being 0.1× travel,
 * double-click-to-reset, scroll-only-when-hovered and one-gesture-one-undo-entry; a
 * component library's slider does none of that, so it would be overridden rather than
 * used, and overriding a control is more work than writing one." This is that control,
 * and the table in §11 is implemented row by row below.
 *
 * The one that shapes the API is the last row of the table's neighbour: **one completed
 * gesture is one undo entry.** So there are two callbacks rather than one. `onInput`
 * fires continuously and drives the render; `onCommit` fires once, on pointer-up or on
 * a numeric commit, and drives history. A slider drag is one history record, not two
 * hundred.
 */

export interface SliderSpec {
  /** The document's own field name — `exposure`, `contrast`. */
  key: string;
  label: string;
  min: number;
  max: number;
  /** §6.1: an omitted key means identity, which for every one of these is zero. */
  default: number;
  /** One `↑`/`↓` press. `Shift` makes it a tenth of this. */
  step: number;
  unit?: string;
  /** Decimal places shown. Fixed, so §10.2's "must not shift the layout while dragging". */
  places?: number;
  /**
   * Added to the stored value for display only.
   *
   * §6.1 on temperature: "The UI shows an absolute figure for the global layer because
   * that is what a photographer reads; the file stores what composes." A Kelvin delta
   * is what composes — stage 2 runs once per layer, so two layers each declaring an
   * absolute 5200 K would describe nothing — and 5200 K is what gets read. This is the
   * one place those two differ, and it differs by an addition rather than by a second
   * representation.
   */
  displayOffset?: number;
  /** Whether a positive value shows a leading `+`. False for an absolute quantity. */
  signed?: boolean;
}

export interface SliderHandlers {
  onInput(key: string, value: number): void;
  onCommit(key: string, value: number): void;
}

export class Slider {
  readonly element: HTMLElement;
  private readonly spec: SliderSpec;
  private readonly handlers: SliderHandlers;
  private readonly track: HTMLElement;
  private readonly fill: HTMLElement;
  private readonly handle: HTMLElement;
  private readonly readout: HTMLButtonElement;
  private current: number;
  /** The value this gesture started from, so a drag that ends where it began commits nothing. */
  private gestureStart = 0;

  constructor(spec: SliderSpec, handlers: SliderHandlers) {
    this.spec = spec;
    this.handlers = handlers;
    this.current = spec.default;

    const row = document.createElement("div");
    row.className = "slider";

    const label = document.createElement("span");
    label.className = "slider-label";
    label.textContent = spec.label;

    this.readout = document.createElement("button");
    this.readout.className = "slider-value numeric";
    this.readout.type = "button";
    // §11: "Click the value | Numeric entry, `Enter` commits, `Esc` cancels".
    this.readout.addEventListener("click", () => this.promptForValue());

    const head = document.createElement("div");
    head.className = "slider-head";
    head.append(label, this.readout);

    this.track = document.createElement("div");
    this.track.className = "slider-track";
    this.track.tabIndex = 0;
    this.track.setAttribute("role", "slider");
    this.track.setAttribute("aria-label", spec.label);

    this.fill = document.createElement("div");
    this.fill.className = "slider-fill";
    this.handle = document.createElement("div");
    this.handle.className = "slider-handle";
    this.track.append(this.fill, this.handle);

    row.append(head, this.track);
    this.element = row;

    // §11: "Double-click label or track | Reset to default".
    for (const target of [label, this.track]) {
      target.addEventListener("dblclick", (e) => {
        e.preventDefault();
        this.gestureStart = this.current;
        this.commit(spec.default);
      });
    }

    this.track.addEventListener("pointerdown", (e) => this.beginDrag(e));
    this.track.addEventListener("keydown", (e) => this.onKey(e));

    // §11: "Scroll over control | Adjust — only when the control is hovered, never when
    // the panel is." Bound to the row rather than the panel, and `passive: false`
    // because the scroll has to be swallowed or the panel moves underneath.
    row.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        e.stopPropagation();
        const by = this.stepFor(e) * (e.deltaY > 0 ? -1 : 1);
        this.gestureStart = this.current;
        this.commit(this.clamp(this.current + by));
      },
      { passive: false },
    );

    this.paint();
  }

  get value(): number {
    return this.current;
  }

  /** Set from outside — loading a document, or undo. Fires nothing. */
  set(value: number): void {
    this.current = this.clamp(value);
    this.paint();
  }

  // ------------------------------------------------------------------ gestures

  private beginDrag(down: PointerEvent): void {
    if (down.button !== 0) return;
    down.preventDefault();
    this.track.focus();
    this.track.setPointerCapture(down.pointerId);
    this.gestureStart = this.current;

    const rect = this.track.getBoundingClientRect();
    const span = this.spec.max - this.spec.min;

    // A press anywhere on the track jumps there — the pointer is already where the
    // user is looking — and the drag continues relatively from that point, so the
    // modifiers below are about the *movement* rather than about the absolute position.
    let last = down.clientX;
    let value = this.clamp(this.spec.min + ((down.clientX - rect.left) / rect.width) * span);
    this.emit(value);

    const move = (e: PointerEvent) => {
      // §11: `Shift` fine — 0.1× travel. `Ctrl` coarse — 10×. Travel means how far the
      // value moves for a given movement of the hand, so it scales the delta.
      const rate = e.shiftKey ? 0.1 : e.ctrlKey ? 10 : 1;
      value = this.clamp(value + ((e.clientX - last) / rect.width) * span * rate);
      last = e.clientX;
      this.emit(value);
    };
    const up = () => {
      this.track.removeEventListener("pointermove", move);
      this.track.removeEventListener("pointerup", up);
      this.track.removeEventListener("pointercancel", up);
      // §11: history commits on pointer-up. One gesture, one entry — and nothing at all
      // if the hand came back to where it started.
      if (this.current !== this.gestureStart) this.handlers.onCommit(this.spec.key, this.current);
    };
    this.track.addEventListener("pointermove", move);
    this.track.addEventListener("pointerup", up);
    this.track.addEventListener("pointercancel", up);
  }

  private onKey(e: KeyboardEvent): void {
    // §11: "`↑` / `↓` | One step; with `Shift`, one fine step".
    let by = 0;
    if (e.key === "ArrowUp" || e.key === "ArrowRight") by = this.stepFor(e);
    else if (e.key === "ArrowDown" || e.key === "ArrowLeft") by = -this.stepFor(e);
    else if (e.key === "Home") return this.jump(this.spec.min);
    else if (e.key === "End") return this.jump(this.spec.max);
    else return;
    e.preventDefault();
    this.gestureStart = this.current;
    this.commit(this.clamp(this.current + by));
  }

  private jump(to: number): void {
    this.gestureStart = this.current;
    this.commit(to);
  }

  private stepFor(e: { shiftKey: boolean; ctrlKey: boolean }): number {
    return this.spec.step * (e.shiftKey ? 0.1 : e.ctrlKey ? 10 : 1);
  }

  private promptForValue(): void {
    const input = document.createElement("input");
    input.className = "slider-entry numeric";
    input.value = (this.current + (this.spec.displayOffset ?? 0)).toFixed(this.spec.places ?? 0);
    input.setAttribute("aria-label", `${this.spec.label} value`);
    this.readout.replaceWith(input);
    input.focus();
    input.select();

    let settled = false;
    const finish = (accept: boolean) => {
      if (settled) return;
      settled = true;
      input.replaceWith(this.readout);
      if (!accept) return;
      const parsed = Number(input.value.replace(",", "."));
      if (!Number.isFinite(parsed)) return;
      this.gestureStart = this.current;
      this.commit(this.clamp(parsed - (this.spec.displayOffset ?? 0)));
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.preventDefault(); finish(true); }
      // §11's global `Esc` cancels the gesture. Stopped here so it does not also
      // dismiss whatever is behind it.
      else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); }
    });
    input.addEventListener("blur", () => finish(true));
  }

  // ------------------------------------------------------------------- plumbing

  private emit(value: number): void {
    this.current = value;
    this.paint();
    this.handlers.onInput(this.spec.key, value);
  }

  /** Change and commit in one — for the gestures that have no "during". */
  private commit(value: number): void {
    this.emit(value);
    if (this.current !== this.gestureStart) this.handlers.onCommit(this.spec.key, this.current);
  }

  private clamp(v: number): number {
    return Math.min(this.spec.max, Math.max(this.spec.min, v));
  }

  private paint(): void {
    const { min, max, default: base, unit, places } = this.spec;
    const at = (this.current - min) / (max - min);
    const zero = (base - min) / (max - min);
    this.handle.style.left = `${at * 100}%`;
    // The fill runs from the default to the value rather than from the left edge, so a
    // centre-zero control reads as a deviation and an exposure of −1 does not look like
    // "a quarter full".
    this.fill.style.left = `${Math.min(at, zero) * 100}%`;
    this.fill.style.width = `${Math.abs(at - zero) * 100}%`;

    const display = this.current + (this.spec.displayOffset ?? 0);
    const shown = display.toFixed(places ?? 0);
    // A real minus sign, and an explicit plus, because §10.2's example is `+0.35 EV`.
    const text =
      this.spec.signed === false
        ? shown.replace("-", "−")
        : display > 0
          ? `+${shown}`
          : shown.replace("-", "−");
    this.readout.textContent = unit ? `${text} ${unit}` : text;
    this.track.setAttribute("aria-valuenow", shown);
    this.element.classList.toggle("at-default", this.current === base);
  }
}
