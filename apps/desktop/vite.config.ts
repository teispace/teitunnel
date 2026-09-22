import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [
    tanstackRouter({ target: "react", autoCodeSplitting: true, quoteStyle: "double" }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // WKWebView on macOS 14+ (D-023); WebView2 and WebKitGTK are newer still.
    target: ["safari17", "chrome120"],
    sourcemap: false,
    reportCompressedSize: true,
    chunkSizeWarningLimit: 250,
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test/setup.ts"],
    restoreMocks: true,
  },
});
