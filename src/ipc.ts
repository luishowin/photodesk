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
 * This is the one large thing that crosses, and it crosses once. Tauri hands raw
 * command responses back as an `ArrayBuffer`, so nothing is parsed on the way.
 */
export const proxyPixels = async (): Promise<Uint16Array> => {
  const raw = await invoke<ArrayBuffer | number[]>("proxy_pixels");
  // Older webviews hand back a plain array; both shapes reach the same texture.
  return raw instanceof ArrayBuffer ? new Uint16Array(raw) : new Uint16Array(raw);
};

export const openOnStart = () => invoke<string | null>("open_on_start");

// ---------------------------------------------------------------------- the plan

/** §6.2's compile. It happens in Rust because §0 says it happens once (see the register). */
export const compile = (document: Document) => invoke<Graph>("compile", { document });

/** Stage 13's derived constants, from the code that derives them for the export. */
export const encodeUniform = async (space: string): Promise<Float32Array> => {
  const raw = await invoke<ArrayBuffer | number[]>("encode_uniform", { space });
  const bytes = raw instanceof ArrayBuffer ? new Uint8Array(raw) : new Uint8Array(raw);
  return new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 4);
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
