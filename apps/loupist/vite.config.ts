import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  // Scan only the app's pages: ignored e2e artifacts hold stale HTML whose failed scan makes Vite reload mid-run.
  optimizeDeps: {
    entries: ["index.html", "preview.html"],
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    include: ["src/**/*.test.{ts,tsx}"],
    environment: "jsdom",
    globals: true,
    setupFiles: "./src/test/setup.ts",
    css: true,
    // Leaves room for the widened testing-library async window under load.
    testTimeout: 60_000,
    // jsdom renders of the whole app are CPU-bound; half the cores keeps each
    // file responsive when the gate's cargo lane shares the machine.
    maxWorkers: "50%",
  },
});
