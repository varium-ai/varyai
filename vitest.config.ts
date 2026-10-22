import path from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  // Tests need JSX conversion, not the production plugin's Fast Refresh hooks.
  // Keep the browser application and Node test environment explicit.
  esbuild: { jsx: "automatic" },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    maxWorkers: 2,
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
