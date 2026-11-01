import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import react from "eslint-plugin-react";
import jsxA11y from "eslint-plugin-jsx-a11y";
import prettier from "eslint-config-prettier";

// ESLint 9+ flat config (replaces the legacy .eslintrc.cjs). Mirrors the
// engine UI repo's setup, adapted to this project (no generated src/types;
// the vitest setup file is src/vitest.setup.ts).
export default tseslint.config(
  {
    ignores: [
      "dist",
      "coverage",
      "playwright-report",
      "test-results",
      ".stryker-tmp",
      "reports",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  // React + JSX-a11y recommended rules — catch the non-interactive-element /
  // missing-keyboard-handler gaps (U1/DX1). Scoped to source files; the new JSX
  // transform means React need not be in scope (jsx-runtime turns that rule off).
  {
    files: ["**/*.{ts,tsx}"],
    ...react.configs.flat.recommended,
    // Pin the React version: eslint-plugin-react@7's auto-detect path is
    // incompatible with ESLint 10's context API and throws on load.
    settings: { react: { version: "19.2" } },
  },
  react.configs.flat["jsx-runtime"],
  jsxA11y.flatConfigs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    settings: {
      // `<Input>` is the project's thin wrapper over a native <input>, so labels
      // wrapping it are properly associated — teach the rule to see it as a control.
      "jsx-a11y": { components: { Input: "input" } },
    },
    rules: {
      // TypeScript already enforces prop typing; the runtime prop-types rule is
      // redundant noise in a TS codebase.
      "react/prop-types": "off",
      // Auto-focusing the primary field when a modal/inline-rename opens is the
      // intended focus-management behavior here (WAI-ARIA dialog pattern), not an
      // accidental page-load focus steal. The rule's blanket ban doesn't fit.
      "jsx-a11y/no-autofocus": "off",
      // Classic react-hooks rules (parity with the previous .eslintrc setup).
      // eslint-plugin-react-hooks@7's "recommended" now also bundles the new
      // React-Compiler rules (refs/immutability/set-state-in-effect); adopting
      // those is a separate decision (they flag existing working code), so they
      // are intentionally not enabled in this config-format migration.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
    },
  },
  // Tests, e2e, the vitest setup file, and root config files are not subject
  // to the react-refresh component-export rule.
  {
    files: [
      "**/*.test.{ts,tsx}",
      "e2e/**",
      "src/vitest.setup.ts",
      "*.config.{ts,js}",
    ],
    rules: { "react-refresh/only-export-components": "off" },
  },
  prettier,
);
