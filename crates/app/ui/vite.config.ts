import { defineConfig } from "vitest/config";

// Tauri expects a fixed port; the webview loads http://localhost:1420 in dev.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", sourcemap: false },
  test: { environment: "jsdom", include: ["src/**/*.test.ts"] },
});
