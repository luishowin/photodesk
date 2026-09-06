import { defineConfig } from "vite";
import { resolve } from "node:path";

/**
 * Builds the §12.2 harness page's entry, which imports the *product's* graph executor
 * and GL layer rather than a copy of them. Separate from `vite.config.ts` because it
 * emits into the spike harness's gitignored `generated/` directory, beside the inputs
 * `cargo test -p photodesk-renderer-spike` writes there.
 */
export default defineConfig({
  build: {
    outDir: resolve(__dirname, "tests/renderer/web/generated"),
    emptyOutDir: false,
    target: "es2022",
    lib: {
      entry: resolve(__dirname, "tests/renderer/web/plan-entry.ts"),
      formats: ["es"],
      fileName: () => "plan.js",
    },
  },
});
