import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,

  // A disable comment that no longer suppresses anything is itself an
  // error, so the per-line opt-outs below cannot outlive their reason.
  { linterOptions: { reportUnusedDisableDirectives: "error" } },

  {
    // Same file scope eslint-config-next's own rule-bearing config objects
    // use, so the react-hooks/@typescript-eslint plugins they register are
    // in scope for this object's rule overrides too.
    files: ["**/*.{js,jsx,mjs,ts,tsx,mts,cts}"],
    rules: {
      // `any` is an error like the rest of typescript-eslint's
      // recommended set (eslint-config-next softens it to a warning).
      "@typescript-eslint/no-explicit-any": "error",

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

      // eslint-plugin-react-hooks 7's React Compiler rules
      // (set-state-in-effect, refs, purity, globals) stay at their default
      // `error`. The codebase has a handful of deliberate, documented
      // patterns they flag: syncing local state from props or the URL in an
      // effect, the "latest ref" pattern, one `Date.now()` stamp per server
      // render, and test harnesses that hand a component's setter to the
      // enclosing `it()`. Each carries its own
      // `eslint-disable-next-line <rule> -- <reason>` rather than a global
      // downgrade, so new code still has to justify the same exemption.
      //
      // exhaustive-deps is only a warning upstream. `npm run lint` fails on
      // any warning (--max-warnings 0), but make it an error here too so an
      // editor shows it as one.
      "react-hooks/exhaustive-deps": "error",
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
