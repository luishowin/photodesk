/**
 * The viewport: one photograph, fitted, with §11's before/after.
 *
 * The chain is drawn entirely into offscreen targets and then **blitted** to the canvas.
 * That is not an extra step for its own sake — it is where the orientation problem gets
 * resolved, and the resolution is one flip at the end rather than a parity that
 * accumulates.
 *
 * `shaders/photodesk/` is authored for wgpu, whose framebuffer origin is top-left, and
 * GL's is bottom-left. **naga already reconciles that**: `engine::glsl` lowers with
 * `ADJUST_COORDINATE_SPACE`, which negates `gl_Position.y`, so a pass in GL writes the
 * same row indices as the same pass in wgpu. That is why §12.2 agrees to the code
 * (`max 0 of 255`) rather than agreeing upside down, and it means a pass is *identity*
 * in index space — one, two or ten of them leave the photograph's first row at
 * framebuffer row 0.
 *
 * What remains is the last step alone: framebuffer row 0 is the canvas's **bottom**, so
 * the source rectangle is read bottom-up. Unconditionally. `preview.ts` used to derive
 * that from the pass count, on the theory that each pass inverted — which made the
 * picture correct only when the count happened to be even, and a photograph with no
 * adjustments (one `encode` pass) presented upside down for the whole of v0.1.
 *
 * The blit earns its place twice over: scaling to fit is free, and §11's split view is a
 * second blit with a narrower rectangle rather than a second shader.
 */

import type { Graph } from "../document/generated/graph";
import { type Programs, type Resources, execute } from "../graph/execute";
import { type Target, displayTarget, dispose, target, texture } from "./gl";

export type View = "edited" | "original" | "split";

export class Preview {
  private readonly gl: WebGL2RenderingContext;
  private readonly canvas: HTMLCanvasElement;
  private readonly programs: Programs;
  private encodeUniform: Float32Array<ArrayBufferLike> = new Float32Array(0);

  private source: WebGLTexture | null = null;
  private targets: [Target, Target] | null = null;
  /** Where the edited frame's output node lands, 8-bit and ready to present. */
  private display: Target | null = null;
  /** The photograph with no edits, drawn once per open — §11's `Space`, held. */
  private original: Target | null = null;
  private width = 0;
  private height = 0;
  lastBlit = "(none)";
  centre = "(unread)";
  probe = true;

  /** Where the image sits in the canvas, in canvas pixels. For hit-testing later. */
  private fitted = { x: 0, y: 0, w: 0, h: 0 };

  /**
   * The context is passed in rather than created here, because the programs have to be
   * linked into it and they are built before there is anything to preview.
   */
  constructor(canvas: HTMLCanvasElement, gl: WebGL2RenderingContext, programs: Programs) {
    this.canvas = canvas;
    this.gl = gl;
    this.programs = programs;
  }

  /**
   * Hand over a freshly decoded proxy, and the plan that renders it unedited.
   *
   * `originalGraph` is the same document with an empty stack, compiled by Rust like any
   * other — §11's "show original" is the photograph as decoded and encoded for the
   * display, which is a plan rather than a special case. It is drawn **once, here**,
   * because it cannot change while a document is open: an edit changes the edited
   * frame, and the thing it is being compared against is the point of the comparison.
   */
  load(
    pixels: Uint16Array,
    width: number,
    height: number,
    encodeUniform: Float32Array,
    originalGraph: Graph,
  ): void {
    const gl = this.gl;
    if (this.source) gl.deleteTexture(this.source);
    if (this.targets) for (const t of this.targets) dispose(gl, t);
    if (this.display) dispose(gl, this.display);
    if (this.original) dispose(gl, this.original);

    this.source = texture(gl, width, height, pixels);
    this.targets = [target(gl, width, height), target(gl, width, height)];
    this.display = displayTarget(gl, width, height);
    this.original = displayTarget(gl, width, height);
    this.width = width;
    this.height = height;
    this.encodeUniform = encodeUniform;

    // Straight into the original's own target, so the edited frame can overwrite the
    // shared one on every draw without touching it.
    execute(this.resources(this.original), originalGraph);
  }

  get loaded(): boolean {
    return this.source !== null;
  }

  private resources(display: Target): Resources {
    return {
      gl: this.gl,
      programs: this.programs,
      source: this.source as WebGLTexture,
      targets: this.targets as [Target, Target],
      display,
      encodeUniform: this.encodeUniform,
    };
  }

  /**
   * Draw the document as it stands, and present it.
   *
   * Returns nothing, deliberately. The obvious thing to return is how long it took, and
   * that number would be CPU submit time — GL commands are asynchronous, so it reads
   * near zero against a 16 ms budget and measures the wrong thing. What §7.3 budgets is
   * the interval between frames, which only the caller running the loop can see.
   */
  render(graph: Graph, view: View, split: number): void {
    if (!this.source || !this.targets || !this.display) return;
    const shown = execute(this.resources(this.display), graph);
    this.present(shown.output, view, split);
  }

  /**
   * One line of what GL thinks, the first time a frame is drawn.
   *
   * A preview that produces nothing produces no exception either — a failed blit or a
   * texture that would not allocate raises a GL error code and draws an empty canvas,
   * which is indistinguishable from a correct render of nothing.
   */
  diagnose(): string {
    const gl = this.gl;
    return [
      `err ${gl.getError()}`,
      `lost ${gl.isContextLost()}`,
      `drawingBuffer ${gl.drawingBufferWidth}×${gl.drawingBufferHeight}`,
      `renderer ${gl.getParameter(gl.RENDERER)}`,
      `version ${gl.getParameter(gl.VERSION)}`,
      `max texture ${gl.getParameter(gl.MAX_TEXTURE_SIZE)}`,
      `proxy ${this.width}×${this.height}`,
      `fitted ${this.fitted.w}×${this.fitted.h} at ${this.fitted.x},${this.fitted.y}`,
      `float colour buffer ${!!gl.getExtension("EXT_color_buffer_float")}`,
      `canvas centre ${this.centre}`,
    ].join("  ·  ");
  }

  /** Fit the image into the canvas and blit it the right way up. */
  private present(editedTarget: Target, view: View, split: number): void {
    const gl = this.gl;
    const dpr = globalThis.devicePixelRatio || 1;
    const cw = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
    const ch = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
    if (this.canvas.width !== cw || this.canvas.height !== ch) {
      this.canvas.width = cw;
      this.canvas.height = ch;
    }

    // §11's `F`, which is also the only zoom v0.1 has: fit, never enlarging past 1:1.
    const scale = Math.min(cw / this.width, ch / this.height, 1);
    const w = Math.round(this.width * scale);
    const h = Math.round(this.height * scale);
    const x = Math.round((cw - w) / 2);
    const y = Math.round((ch - h) / 2);
    this.fitted = { x, y, w, h };

    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    gl.viewport(0, 0, cw, ch);
    // §10.1: the surround is #333333, not black — a near-black surround makes images
    // read brighter than they are and you systematically under-expose.
    gl.clearColor(0x33 / 255, 0x33 / 255, 0x33 / 255, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    const blit = (src: Target, dx0: number, dx1: number) => {
      const sx0 = Math.round(((dx0 - x) / Math.max(w, 1)) * this.width);
      const sx1 = Math.round(((dx1 - x) / Math.max(w, 1)) * this.width);
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, src.framebuffer);
      // Read the source bottom-up, which is what presents it upright. See the note on
      // this class: the flip is one step at the end, not a parity that accumulates.
      gl.blitFramebuffer(
        sx0, this.height,
        sx1, 0,
        dx0, y,
        dx1, y + h,
        gl.COLOR_BUFFER_BIT,
        gl.LINEAR,
      );
      this.lastBlit = `src ${sx0},${this.height}→${sx1},0 ` +
        `dst ${dx0},${y}→${dx1},${y + h} err ${gl.getError()}`;
    };

    if (view === "original") {
      blit(this.original as Target, x, x + w);
    } else if (view === "split") {
      const at = x + Math.round(w * split);
      blit(this.original as Target, x, at);
      blit(editedTarget, at, x + w);
    } else {
      blit(editedTarget, x, x + w);
    }
    // One pixel of what was just presented, from the middle of where the photograph
    // landed — read before the read binding is dropped. Kept rather than deleted with
    // the rest of the scaffolding, because the difference between "a frame was drawn"
    // and "a frame with a photograph in it was drawn" is the whole of one long
    // debugging session: a black texture, a successful blit, and no error anywhere.
    if (this.probe) {
      const px = new Uint8Array(4);
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
      gl.readPixels(x + (w >> 1), ch - 1 - (y + (h >> 1)), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
      this.centre = `${px[0]},${px[1]},${px[2]},${px[3]}`;
      this.probe = false;
    }
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
  }

  /** Where the photograph is on screen, in CSS pixels — for the split handle. */
  get rect(): { x: number; y: number; w: number; h: number } {
    const dpr = globalThis.devicePixelRatio || 1;
    return {
      x: this.fitted.x / dpr,
      y: this.fitted.y / dpr,
      w: this.fitted.w / dpr,
      h: this.fitted.h / dpr,
    };
  }
}
