import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,

  {
    // Same file scope eslint-config-next's own rule-bearing config objects
    // use, so the react-hooks/@typescript-eslint plugins they register are
    // in scope for this object's rule overrides too.
    files: ["**/*.{js,jsx,mjs,ts,tsx,mts,cts}"],
    rules: {
      // This codebase deliberately uses `any` in a handful of narrow spots
      // (e.g. bridging untyped third-party payloads); each such use should
      // still be reviewed on its own, so this is a warning rather than a
      // hard error.
      "@typescript-eslint/no-explicit-any": "warn",

      // Unused function parameters are common in this codebase for
      // documenting a callback's full signature even when only some
      // arguments are used (Mantine render-prop callbacks, event handlers,
      // etc.). Still flag genuinely unused local variables/imports as
      // errors, but don't error on unused args prefixed with `_`.
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          args: "after-used",
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],

      // eslint-plugin-react-hooks 7 (pulled in by eslint-config-next 16)
      // bundles a set of new, much stricter "React Compiler" purity rules
      // as errors by default: no setState calls in an effect body, no
      // reading/writing a ref during render, no impure calls (Date.now(),
      // etc.) during render. This codebase predates those rules and uses
      // several deliberate, documented patterns they flag as violations --
      // e.g. syncing local state from props/URL params in an effect
      // (HistoryRangePicker), the "latest ref" pattern for reading a
      // ref's current value from a cleanup/callback without adding it as
      // an effect dependency (AutoRefresh, TimeFilterInput), and a fixed
      // `Date.now()` snapshot taken once per server render so SSR and
      // hydration agree (app/lines/[id]/page.tsx and siblings, which
      // comment on this explicitly). None of this app's components are
      // compiled with the React Compiler, so these are real-but-narrow
      // style guidance rather than functional bugs today. Downgraded to
      // warnings rather than mass-rewriting every flagged call site or
      // scattering dozens of disable comments across established code;
      // new code should still avoid triggering them.
      "react-hooks/set-state-in-effect": "warn",
      "react-hooks/refs": "warn",
      "react-hooks/purity": "warn",

      // Only ever seen in this codebase's test harnesses: a module-scoped
      // `let` reassigned inside a mounted test component so an outer
      // `it()` block can drive the component's state (see
      // AutoRefresh.test.tsx, ConnectivityMonitor.test.tsx,
      // app/error.test.tsx). A real anti-pattern in application code, but
      // a deliberate, repeated convention in tests, so warn rather than
      // error.
      "react-hooks/globals": "warn",
    },
  },

  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // This repo's own build/output paths:
    "coverage/**",
    "playwright-report/**",
    "test-results/**",
  ]),
]);

export default eslintConfig;
