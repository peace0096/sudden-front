import { existsSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, type Plugin } from "vite";

/**
 * public/art/_candidates/ holds local-only picks for the ?art&candidates gallery
 * (tools/art/stage-candidates.mjs). Vite copies all of public/ into the build, so this removes
 * that copy again: only files registered in public/art/LICENSES.json ever ship. The build fails if
 * the copy is still there afterwards.
 */
function dropArtCandidates(): Plugin {
  let outDir = "";
  return {
    name: "drop-art-candidates",
    apply: "build",
    configResolved(config) {
      outDir = resolve(config.root, config.build.outDir);
    },
    closeBundle() {
      const candidates = resolve(outDir, "art", "_candidates");
      rmSync(candidates, { recursive: true, force: true });
      if (existsSync(candidates)) throw new Error(`${candidates} is still there after removing it; unregistered candidate art must not ship`);
    },
  };
}

export default defineConfig({
  plugins: [dropArtCandidates()],
  server: {
    port: 5173,
    proxy: { "/ws": { target: `ws://localhost:${process.env.PORT ?? 8800}`, ws: true } },
  },
  build: { chunkSizeWarningLimit: 1200 },
});
