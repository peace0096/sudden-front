import { defineConfig } from "vite";

export default defineConfig({
  server: {
    port: 5173,
    proxy: { "/ws": { target: `ws://localhost:${process.env.PORT ?? 8800}`, ws: true } },
  },
  build: { chunkSizeWarningLimit: 1200 },
});
