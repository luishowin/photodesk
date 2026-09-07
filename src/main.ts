/**
 * PhotoDesk, one screen (§10.3).
 *
 * ```
 * ┌──────────────────────────────────────────────────────────┐
 * │  ‹ Photos          IMG_4821.HEIC              Export     │
 * ├──────────────────────────────────────────────────────────┤
 * │                    the photograph                        │
 * │                  (#333333 surround)                      │
 * ├──────────────────────────────────────────────────────────┤
 * │  Crop   Light   Color   Detail   Effects   Masks   AI     │
 * ├──────────────────────────────────────────────────────────┤
 * │  Exposure        ──────────●───────────      +0.35 EV    │
 * └──────────────────────────────────────────────────────────┘
 * ```
 *
 * The render loop is the shape §7.3 asks for: a slider drag marks the frame dirty and
 * `requestAnimationFrame` draws at most one frame per refresh, so a drag that outruns
 * the GPU degrades to fewer frames rather than to a queue of stale ones. Nothing here
 * awaits IPC on that path — the whole reason §7.2 puts the preview in the webview.
 */

import "./design/tokens.css";
import "./app.css";

import type { Document } from "./document/generated/document";
import type { Graph } from "./document/generated/graph";
import { History } from "./document/history";
import { adjustments, isEdited, setParameter, withoutEdits } from "./document/edit";
import { Preview, type View } from "./canvas/preview";
import { Program, context } from "./canvas/gl";
import { TABS, buildLightPanel } from "./panels/light";
import type { Slider } from "./panels/slider";
import * as ipc from "./ipc";

// ------------------------------------------------------------------------- state

interface Session {
  opened: ipc.Opened;
  history: History;
  /** The compiled plan for the current document. Recompiled in Rust on every change. */
  graph: Graph;
  /** Whether the document differs from what is on disk. */
  dirty: boolean;
}

let session: Session | null = null;
let preview: Preview | null = null;
let view: View = "edited";
let splitAt = 0.5;
let needsFrame = false;
/**
 * Frames per second while something is moving, which is how §7.3 states its requirement:
 * "60 fps at proxy with up to 6 layers".
 *
 * A rate rather than a duration, and that is the second correction this readout has
 * needed. The time `render` takes to return is CPU submit time — GL commands are
 * asynchronous — so it reads 0.00 ms against a 16 ms budget: a confident number
 * measuring the wrong thing. The interval between drawn frames is real, but the loop is
 * `requestAnimationFrame`-driven, so it cannot go below the display's refresh interval
 * and reads 17.0 ms against "budget 16.0 ms" while comfortably meeting it. The rate has
 * neither problem: 60 is the ceiling, and anything below it is the signal.
 */
let fps = 0;
let lastDrawnAt = 0;
let everDrew = 0;

// -------------------------------------------------------------------- the chrome

const root = document.createElement("div");
root.className = "app";
document.body.append(root);

const header = document.createElement("header");
const openButton = button("Open…", () => void chooseAndOpen());
const title = document.createElement("div");
title.className = "title";
title.textContent = "No photograph open";
const exportButton = button("Export", () => void runExport());
exportButton.disabled = true;
header.append(openButton, title, exportButton);

const stage = document.createElement("div");
stage.className = "stage";
const canvas = document.createElement("canvas");
stage.append(canvas);

const empty = document.createElement("div");
empty.className = "empty";
empty.innerHTML =
  "<p>Open a photograph.</p>" +
  "<p class='dim'>HEIF, JPEG or PNG. The file on disk is never modified.</p>";
stage.append(empty);

const tabs = document.createElement("nav");
tabs.className = "tabs";
for (const tab of TABS) {
  const el = document.createElement("button");
  el.type = "button";
  el.className = tab.version === null ? "tab active" : "tab";
  el.append(tab.name);
  if (tab.version !== null) {
    // §9.4's posture, which extends to anything not built yet: a missing capability
    // greys out **with a reason** rather than vanishing or lying. The reason has to be
    // on the tab, not in a tooltip — a disabled control with a hidden explanation is
    // indistinguishable from a broken one, which is exactly how it was read.
    el.disabled = true;
    const version = document.createElement("span");
    version.className = "tab-version";
    version.textContent = `v${tab.version}`;
    el.append(version);
    el.title = `${tab.name} arrives in v${tab.version}. This is v0.1.`;
  }
  tabs.append(el);
}

const handlers = {
  onInput: (key: string, value: number) => {
    if (!session) return;
    // During a gesture: change the document, recompile, draw. No history entry.
    const next = setParameter(session.history.current, key, value);
    session.history.preview(next);
    void recompileAndDraw(next);
  },
  onCommit: (key: string, value: number) => {
    if (!session) return;
    // §11: one completed gesture is one undo entry, committed on pointer-up.
    const next = setParameter(session.history.current, key, value);
    session.history.commit(next);
    session.dirty = true;
    void recompileAndDraw(next);
    void persist();
  },
};

const light = buildLightPanel(handlers);
const sliders: Map<string, Slider> = light.sliders;

const status = document.createElement("div");
status.className = "status numeric";

const panels = document.createElement("section");
panels.className = "panels";
panels.append(light.element, status);

const notices = document.createElement("div");
notices.className = "notices";

root.append(header, stage, tabs, notices, panels);

function button(label: string, onClick: () => void): HTMLButtonElement {
  const el = document.createElement("button");
  el.type = "button";
  el.className = "chrome-button";
  el.textContent = label;
  el.addEventListener("click", onClick);
  return el;
}

// ------------------------------------------------------------------- the pipeline

/**
 * Recompile in Rust and mark the frame dirty.
 *
 * §0: the graph is compiled once, in Rust — so a parameter change is a round trip. It
 * is a small JSON document rather than a frame, and it does not block the draw: the
 * preview keeps rendering the last plan until the new one lands, which is what §11's
 * "continuous update, never blocks" means with an async compile behind it.
 */
let compileToken = 0;
async function recompileAndDraw(document: Document): Promise<void> {
  if (!session) return;
  const token = ++compileToken;
  try {
    const graph = await ipc.compile(document);
    // A compile that finished after a newer one started is stale. Dropping it here is
    // what stops a fast drag from rendering out of order.
    if (token !== compileToken || !session) return;
    session.graph = graph;
    invalidate();
  } catch (e) {
    report(e);
  }
}

function invalidate(): void {
  needsFrame = true;
}

function frame(): void {
  if (needsFrame && session && preview?.loaded) {
    needsFrame = false;
    try {
      preview.render(session.graph, view, splitAt);
      const now = performance.now();
      // Only while frames are consecutive — two draws a second apart are not a frame
      // rate, they are two separate edits. Smoothed, because a single late frame is
      // noise and the number is being read while a hand is moving.
      const gap = now - lastDrawnAt;
      if (lastDrawnAt && gap < 200) {
        const instant = 1000 / gap;
        fps = fps === 0 ? instant : fps * 0.8 + instant * 0.2;
      }
      lastDrawnAt = now;
      if (everDrew === 0) { everDrew = 1; invalidate(); }
      else if (everDrew === 1) {
        everDrew = 2;
        void ipc.log(
          `first frame: canvas ${canvas.width}×${canvas.height}  ·  ${preview.diagnose()}` +
            `  ·  blit ${preview.lastBlit}`,
          "info",
        );
      }
      paintStatus();
    } catch (e) {
      report(e);
    }
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

// A resize changes the fit, and the fit is computed at present time.
globalThis.addEventListener("resize", invalidate);

// ---------------------------------------------------------------------- opening

async function chooseAndOpen(): Promise<void> {
  try {
    const path = await ipc.pickPhotograph();
    if (path) await open(path);
  } catch (e) {
    report(e);
  }
}

async function open(path: string): Promise<void> {
  const started = performance.now();
  try {
    title.textContent = "Opening…";
    // §7.1: the proxy is min(2 × viewport longest edge, source longest edge).
    const dpr = globalThis.devicePixelRatio || 1;
    const viewport = Math.round(Math.max(stage.clientWidth, stage.clientHeight) * dpr);
    const opened = await ipc.openImage(path, Math.max(viewport, 1));

    const [pixels, encodeUniform] = await Promise.all([
      ipc.proxyPixels(opened.proxyWidth * opened.proxyHeight * 4),
      ipc.encodeUniform(opened.document.output.colorspace),
    ]);
    const originalGraph = await ipc.compile(withoutEdits(opened.document));
    const graph = await ipc.compile(opened.document);

    const canvasView = await ensurePreview();
    canvasView.load(pixels, opened.proxyWidth, opened.proxyHeight, encodeUniform, originalGraph);

    session = { opened, history: new History(opened.document), graph, dirty: false };
    for (const [key, slider] of sliders) {
      const value = (adjustments(opened.document) as Record<string, number | null | undefined>)[key];
      slider.set(value ?? 0);
    }

    void ipc.log(
      `opened ${opened.fileName} in ${(performance.now() - started).toFixed(0)} ms: ` +
        `${opened.width}×${opened.height}, proxy ` +
        `${opened.proxyWidth}×${opened.proxyHeight}, ${pixels.length * 2} bytes, ` +
        `${opened.sourceSpace}, plan ${session?.graph.nodes.length ?? "?"} nodes`,
      "info",
    );
    title.textContent = opened.fileName;
    exportButton.disabled = false;
    empty.style.display = "none";
    canvas.style.display = "block";
    for (const notice of opened.notices) report(notice, "notice");
    if (opened.readOnly) report(opened.readOnly, "notice");
    invalidate();
  } catch (e) {
    title.textContent = "No photograph open";
    report(e);
  }
}

/**
 * The context and the two programs, built once.
 *
 * The shaders come from Rust, lowered at startup (§7.2) — so this is the one await
 * between opening a file and seeing it, and it happens once per session rather than
 * once per photograph.
 */
let gl: WebGL2RenderingContext | null = null;
let programs: { adjust: Program; encode: Program } | null = null;

async function ensurePreview(): Promise<Preview> {
  if (preview) return preview;
  gl ??= context(canvas);
  if (!programs) {
    const src = await ipc.shaders();
    programs = {
      adjust: new Program(gl, src.adjust_vert, src.adjust_frag, "adjust"),
      encode: new Program(gl, src.encode_vert, src.encode_frag, "encode"),
    };
  }
  preview = new Preview(canvas, gl, programs);
  return preview;
}

// ------------------------------------------------------------------- persistence

/** §6.1's sidecar, written after a gesture settles. The source file is never touched. */
async function persist(): Promise<void> {
  if (!session || session.opened.readOnly) return;
  try {
    await ipc.saveSidecar(session.history.current);
    session.dirty = false;
    paintStatus();
  } catch (e) {
    report(e);
  }
}

async function runExport(): Promise<void> {
  if (!session) return;
  const document = session.history.current;
  const extension = document.output.format === "png" ? "png" : "jpg";
  const base = session.opened.fileName.replace(/\.[^.]+$/, "");
  try {
    const path = await ipc.pickExportPath(`${base}.${extension}`, [extension]);
    if (!path) return;
    exportButton.disabled = true;
    exportButton.textContent = "Exporting…";
    const done = await ipc.exportImage(document, path);
    report(
      `Exported ${done.width}×${done.height}, ${(done.bytes / 1e6).toFixed(1)} MB, ${done.millis} ms`,
      "notice",
    );
  } catch (e) {
    report(e);
  } finally {
    exportButton.disabled = false;
    exportButton.textContent = "Export";
  }
}

// ------------------------------------------------------------------- §11's keys

globalThis.addEventListener("keydown", (e) => {
  const typing = e.target instanceof HTMLInputElement;
  if (typing) return;

  // §11: "`Space` (hold) | Show original. Release returns. **Never a toggle.**"
  if (e.code === "Space" && !e.repeat) {
    e.preventDefault();
    view = "original";
    invalidate();
    return;
  }
  if (e.key === "\\") {
    e.preventDefault();
    view = view === "split" ? "edited" : "split";
    invalidate();
    return;
  }
  if (e.key === "f" || e.key === "F") {
    // §11's "fit to window", which is the only zoom v0.1 has — so this is a redraw.
    invalidate();
    return;
  }
  if (e.key === "Escape") {
    view = "edited";
    invalidate();
    return;
  }
  const accel = e.ctrlKey || e.metaKey;
  if (accel && (e.key === "z" || e.key === "Z")) {
    e.preventDefault();
    step(e.shiftKey ? "redo" : "undo");
    return;
  }
  if (accel && (e.key === "e" || e.key === "E")) {
    e.preventDefault();
    void runExport();
    return;
  }
  if (accel && (e.key === "o" || e.key === "O")) {
    e.preventDefault();
    void chooseAndOpen();
  }
});

globalThis.addEventListener("keyup", (e) => {
  if (e.code === "Space") {
    view = view === "original" ? "edited" : view;
    invalidate();
  }
});

// The split handle follows the pointer while the split view is showing. §11 gives `\`
// the toggle and says nothing about where the seam sits, so it sits under the hand.
stage.addEventListener("pointermove", (e) => {
  if (view !== "split" || !preview) return;
  const rect = preview.rect;
  const box = stage.getBoundingClientRect();
  if (rect.w <= 0) return;
  splitAt = Math.min(1, Math.max(0, (e.clientX - box.left - rect.x) / rect.w));
  invalidate();
});

function step(direction: "undo" | "redo"): void {
  if (!session) return;
  const document = direction === "undo" ? session.history.undo() : session.history.redo();
  if (!document) return;
  const params = adjustments(document) as Record<string, number | null | undefined>;
  for (const [key, slider] of sliders) slider.set(params[key] ?? 0);
  session.dirty = true;
  void recompileAndDraw(document);
  void persist();
}

// ------------------------------------------------------------------- the readout

function paintStatus(): void {
  if (!session) {
    status.textContent = "";
    return;
  }
  const o = session.opened;
  const lines = [
    `${o.width}×${o.height}  ·  proxy ${o.proxyWidth}×${o.proxyHeight}`,
    `${o.sourceSpace}  ·  ${o.colourTag}  ·  ${o.bitDepth}-bit`,
    fps > 0 ? `${fps.toFixed(0)} fps  ·  §7.3 wants 60 at proxy` : "—  ·  §7.3 wants 60 at proxy",
  ];
  // §4 asks for the gain-map discard to be visible rather than assumed, and this is
  // where "says so out loud" lands in the product.
  if (o.gainMap) lines.push("HDR gain map present — not applied (§4)");
  if (o.alphaComposited) lines.push("alpha composited onto white");
  if (isEdited(session.history.current)) lines.push(session.dirty ? "unsaved" : "saved");
  status.textContent = lines.join("\n");
}

// --------------------------------------------------------------------- notices

function report(what: unknown, kind: "error" | "notice" = "error"): void {
  const text = what instanceof Error ? what.message : String(what);
  void ipc.log(text, kind === "error" ? "error" : "info");
  const el = document.createElement("div");
  el.className = `notice ${kind}`;
  el.textContent = text;
  // §11: "No modal progress dialogs." A notice is dismissible and never blocks.
  el.addEventListener("click", () => el.remove());
  notices.append(el);
  if (kind === "notice") setTimeout(() => el.remove(), 6000);
}

// ------------------------------------------------------------------------ start

canvas.style.display = "none";
if (ipc.inTauri) {
  void ipc.openOnStart().then((path) => {
    if (path) void open(path);
  });
} else {
  report(
    "This page is not running inside the PhotoDesk window, so the Rust core is not " +
      "reachable. Run the application rather than the dev server.",
  );
}
