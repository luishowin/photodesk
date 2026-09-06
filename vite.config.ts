import { defineConfig } from "vite";

// Tauri serves `dist/` from a custom protocol, so every asset reference has to be
// relative — an absolute `/assets/...` resolves against the protocol root and 404s in
// the packaged app while working perfectly in `vite dev`.
export default defineConfig({
  base: "./",
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
