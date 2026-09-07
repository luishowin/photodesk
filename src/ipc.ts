/**
 * The Rust boundary, in one file.
 *
 * Everything the front end cannot compute for itself comes through here, and the list
 * is short on purpose (§7.2): the shaders, the compiled graph, stage 13's uniform, the
 * proxy's pixels once, and the two operations that touch the disk. A slider drag
 * crosses none of it.
 *
 * Reached through `window.__TAURI__` rather than `@tauri-apps/api`, because §10.3 froze
 * zero runtime dependencies and that package would be the one — a wrapper around a
 * function the webview already has.
 */

import type { Document } from "./document/generated/document";
import type { Graph } from "./document/generated/graph";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

const tauri = (globalThis as { __TAURI__?: { core: { invoke: Invoke } } }).__TAURI__;

/**
 * Running outside a Tauri window — `npm run dev` in a browser — is a real state rather
 * than a bug, and it is worth failing legibly in. Every command below rejects with a
 * sentence instead of a `TypeError` on `undefined`.
 */
export const inTauri = tauri !== undefined;

const invoke: Invoke = (command, args) => {
  if (!tauri) {
    return Promise.reject(
      new Error(
        `\`${command}\` needs the Rust core, and this page is not running inside the ` +
          `PhotoDesk window. Build the front end and run the app rather than the dev server.`,
      ),
    );
  }
  return tauri.core.invoke(command, args);
};

// ------------------------------------------------------------------- the shaders

export interface Shaders {
  adjust_vert: string;
  adjust_frag: string;
  encode_vert: string;
  encode_frag: string;
  uniforms: string[];
}

/** §7.2's preview half: `shaders/photodesk/`, lowered to GLSL ES 3.00 by naga. */
export const shaders = () => invoke<Shaders>("shaders");

// ---------------------------------------------------------------------- the file

export interface Opened {
  path: string;
  fileName: string;
  width: number;
  height: number;
  proxyWidth: number;
  proxyHeight: number;
  sourceSpace: string;
  colourTag: string;
  bitDepth: number;
  gainMap: boolean;
  alphaComposited: boolean;
  orientation: number;
  document: Document;
  fromSidecar: boolean;
  notices: string[];
  readOnly: string | null;
}

export const openImage = (path: string, viewport: number) =>
  invoke<Opened>("open_image", { path, viewport });

/**
 * The proxy as RGBA f16 — the exact array `texImage2D` wants for `RGBA16F`.
 *
 * This is the one large thing that crosses, and it crosses once.
 *
 * **Which shape it arrives in is worth two separate paragraphs, because getting it
 * wrong cost a black canvas once and seven seconds a photograph once.**
 *
 * **The bytes are reinterpreted, never copied element-wise, and that distinction cost a
 * black canvas.** A Tauri command returning `Response` does not necessarily arrive as
 * an `ArrayBuffer` — on the fallback transport below it is a byte array — and
 * `new Uint16Array(bytesArray)` does not
 * reinterpret pairs of bytes as 16-bit values — it builds an array twice as long with
 * one byte's *value* in each slot. Every f16 is then a denormal near zero, so the
 * texture is black; and `texImage2D` accepts a buffer that is too long without
 * complaint, so nothing anywhere raises an error. See `bytesOf`.
 *
 * **And an `ArrayBuffer` is the shape that means the transport is the fast one.** Tauri
 * answers `invoke` over a `fetch` to its own `ipc:` scheme, and falls back to
 * `postMessage` — where a byte vector is serialised as a JSON array of numbers — the
 * first time that fetch is refused, for the rest of the session, having said so only
 * to a console nobody can read. 97 MB of proxy is 6.8 seconds that way and 0.77 the
 * other. So the array shape is handled and *reported*: it is not incorrect, it is the
 * visible end of something upstream being wrong.
 */
export const proxyPixels = async (expected: number): Promise<Uint16Array> => {
  const raw = await invoke<ArrayBuffer | ArrayBufferView | number[]>("proxy_pixels");
  // Correct in every shape, and fast in only one. See `bytesOf` for the correctness
  // half; this is the speed half, and it is checked because the slow shape is what
  // arrives when something *else* is wrong.
  if (Array.isArray(raw)) {
    void log(
      `the proxy arrived as an array of ${raw.length} numbers rather than an ArrayBuffer. ` +
        `That is Tauri's postMessage fallback, which it takes silently and permanently ` +
        `after its fetch-based IPC is refused once — the usual cause is a CSP without ` +
        `\`connect-src ipc:\`. The picture will be correct and opening it will take about ` +
        `seven seconds a photograph instead of one.`,
      "error",
    );
  }
  const bytes = bytesOf(raw);
  const pixels = new Uint16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength >> 1);
  if (pixels.length !== expected) {
    throw new Error(
      `the proxy arrived as ${pixels.length} samples where ${expected} were expected. ` +
        `Too few would have been a GL error; too many is accepted silently and draws ` +
        `a black photograph, which is why this is checked here rather than noticed later.`,
    );
  }
  return pixels;
};

/**
 * Whatever shape the webview handed back, as bytes.
 *
 * Three shapes are possible depending on the webview and the Tauri version — an
 * `ArrayBuffer`, a typed-array view over one, or a plain array of byte values — and the
 * only one that is safe to assume is none of them.
 */
function bytesOf(raw: ArrayBuffer | ArrayBufferView | number[]): Uint8Array {
  if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
  if (ArrayBuffer.isView(raw)) {
    return new Uint8Array(raw.buffer, raw.byteOffset, raw.byteLength);
  }
  return Uint8Array.from(raw);
}

export const openOnStart = () => invoke<string | null>("open_on_start");

/**
 * Put a sentence on the process's stderr.
 *
 * A notice in the window is only visible to whoever is looking at the window, and the
 * failures worth reading are the ones where the window is showing nothing.
 */
export const log = (message: string, level: "error" | "info" = "error") => {
  if (!tauri) return Promise.resolve();
  return tauri.core.invoke<void>("log", { message, level }).catch(() => undefined);
};

// ---------------------------------------------------------------------- the plan

/** §6.2's compile. It happens in Rust because §0 says it happens once (see the register). */
export const compile = (document: Document) => invoke<Graph>("compile", { document });

/** Stage 13's derived constants, from the code that derives them for the export. */
export const encodeUniform = async (space: string): Promise<Float32Array> => {
  const bytes = bytesOf(
    await invoke<ArrayBuffer | ArrayBufferView | number[]>("encode_uniform", { space }),
  );
  return new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength >> 2);
};

// ------------------------------------------------------------------ the two writes

export const saveSidecar = (document: Document) =>
  invoke<string>("save_sidecar", { document });

export interface Exported {
  path: string;
  bytes: number;
  width: number;
  height: number;
  millis: number;
}

export const exportImage = (document: Document, path: string) =>
  invoke<Exported>("export_image", { document, path });

// --------------------------------------------------------------------- the dialogs

interface DialogFilter {
  name: string;
  extensions: string[];
}

/**
 * Tauri's dialog plugin, reached the same way as `invoke` and for the same reason.
 *
 * The formats offered are the ones `decode.rs` actually reads. Listing a format the
 * decoder does not handle turns "PhotoDesk cannot open this" into "PhotoDesk is
 * broken", which is the same distinction §16 #13's named error exists to preserve.
 */
export const READABLE: DialogFilter[] = [
  { name: "Photographs", extensions: ["heic", "heif", "avif", "jpg", "jpeg", "png"] },
];

interface DialogApi {
  open(options: { multiple: false; filters: DialogFilter[] }): Promise<string | null>;
  save(options: { defaultPath?: string; filters: DialogFilter[] }): Promise<string | null>;
}

const dialog = () => {
  const api = (globalThis as { __TAURI__?: { dialog?: DialogApi } }).__TAURI__?.dialog;
  if (!api) throw new Error("the file dialog is only available inside the PhotoDesk window");
  return api;
};

export const pickPhotograph = () => dialog().open({ multiple: false, filters: READABLE });

export const pickExportPath = (defaultPath: string, extensions: string[]) =>
  dialog().save({ defaultPath, filters: [{ name: "Export", extensions }] });
