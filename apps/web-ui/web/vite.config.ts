import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import path from "node:path";

// The ts-rs contract is read straight from the engine crate — no vendored
// copy, so it can never drift from the Rust source (monorepo spec D4).
// tsconfig.json carries the matching `paths` entry for tsc.
const here = path.dirname(fileURLToPath(import.meta.url));
const bindingsDir = path.resolve(
  here,
  "../../../crates/cairn-contract/bindings",
);

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@cairn-contract": bindingsDir },
  },
  server: {
    // Vite refuses to serve files outside the project root by default;
    // pluginValues.ts is a runtime import that lives in the engine crate.
    fs: { allow: [".", bindingsDir] },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/vitest.setup.ts"],
    exclude: ["e2e/**", "node_modules/**"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json-summary", "lcov"],
      reportsDirectory: "./coverage",
      exclude: ["e2e/**", "**/*.config.*", "**/vitest.setup.ts", "dist/**"],
    },
  },
});
