import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: { $lib: fileURLToPath(new URL("./src/lib", import.meta.url)) },
  },
  clearScreen: false,
  server: {
    // localhost resolves to ::1 for WebKitGTK while Vite picks 127.0.0.1: the page stays blank.
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: { target: "esnext", outDir: "dist", emptyOutDir: true },
  // Stores are tested on Node: runes compile to plain values there, which the logic needs.
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
