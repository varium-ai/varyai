import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { rendererChunk } from "./scripts/renderer-chunks";

export default defineConfig(() => {
  const plugins = [react()];
  return {
    root: "src",
    plugins,
    base: "./",
    build: {
      outDir: "../dist",
      emptyOutDir: true,
      rollupOptions: {
        output: {
          manualChunks: rendererChunk,
        },
      },
    },
    server: {
      port: 3000,
      strictPort: true,
    },
    resolve: {
      alias: {
        "@": path.resolve(__dirname, "./src"),
      },
    },
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_"],
  };
});
