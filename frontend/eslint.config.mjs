import comments from '@eslint-community/eslint-plugin-eslint-comments/configs';
import { defineConfig, globalIgnores } from 'eslint/config';
import nextVitals from 'eslint-config-next/core-web-vitals';
import nextTs from 'eslint-config-next/typescript';
import tseslint from 'typescript-eslint';

// The plain-JS files tsconfig.json skips (allowJs: false), split by the
// tsconfig that type-checks them (see `npm run typecheck:scripts`).
const SERVICE_WORKER_FILES = [
  'public/sw.js',
  'public/sw-cache-rules.js',
  'public/sw-cache-rules.test.js',
  'types/service-worker.d.ts',
];
// Vitest suites, their shared helpers and setup, and the Playwright specs.
const TEST_FILES = [
  '**/*.test.{ts,tsx}',
  'test/**/*.{ts,tsx}',
  'e2e/**/*.ts',
  'vitest.setup.ts',
  'vitest.setup.test.ts',
  'vitest.config.ts',
  'playwright.config.ts',
];
const NODE_SCRIPT_FILES = [
  'scripts/stamp-sw-version.mjs',
  'e2e/screenshots/take-screenshots.mjs',
  'e2e/screenshots/_interactive-shots.mjs',
  'eslint.config.mjs',
  'next.config.mjs',
  'next.config.test.js',
  'postcss.config.cjs',
];

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,

  // A disable comment that no longer suppresses anything is itself an
  // error, so the per-line opt-outs below cannot outlive their reason.
  { linterOptions: { reportUnusedDisableDirectives: 'error' } },

  // Every disable comment names its rules and says why:
  // `// eslint-disable-next-line rule -- reason`.
  comments.recommended,
  {
    rules: {
      '@eslint-community/eslint-comments/require-description': 'error',
      '@eslint-community/eslint-comments/no-unlimited-disable': 'error',
    },
  },

  {
    // Same file scope eslint-config-next's own rule-bearing config objects
    // use, so the react-hooks/@typescript-eslint plugins they register are
    // in scope for this object's rule overrides too.
    files: ['**/*.{js,jsx,mjs,ts,tsx,mts,cts}'],
    rules: {
      // `any` is an error like the rest of typescript-eslint's
      // recommended set (eslint-config-next softens it to a warning).
      '@typescript-eslint/no-explicit-any': 'error',

      // Unused function parameters are common in this codebase for
      // documenting a callback's full signature even when only some
      // arguments are used (Mantine render-prop callbacks, event handlers,
      // etc.). Still flag genuinely unused local variables/imports as
      // errors, but don't error on unused args prefixed with `_`.
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          args: 'after-used',
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
          // `const { KEY, ...rest } = obj` to omit a key.
          ignoreRestSiblings: true,
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
      'react-hooks/exhaustive-deps': 'error',
    },
  },

  // TypeScript: `import type` for type-only imports, which
  // verbatimModuleSyntax (tsconfig.base.json) keeps as written.
  {
    files: ['**/*.{ts,tsx,mts,cts}'],
    rules: {
      // disallowTypeAnnotations off: Vitest's `vi.importActual<typeof
      // import('m')>()` partial-mock idiom and the global (non-module)
      // types/service-worker.d.ts both need inline `import()` types.
      '@typescript-eslint/consistent-type-imports': ['error', { disallowTypeAnnotations: false }],
    },
  },

  // typescript-eslint's strictest type-aware presets for the app's
  // TypeScript, as the guide recommends. projectService lints each file
  // against tsconfig.json; the service-worker types have their own program
  // below.
  {
    files: ['**/*.{ts,tsx,mts,cts}'],
    ignores: SERVICE_WORKER_FILES,
    extends: [tseslint.configs.strictTypeChecked, tseslint.configs.stylisticTypeChecked],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    rules: {
      // A number always stringifies losslessly. Nullish, any, boolean and
      // objects stay banned (the guide's setting, as for the scripts below).
      '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
      // `onClick={() => setOpen(true)}`: an arrow shorthand that returns a
      // setter's `void` is React's everyday idiom, not a confused void. The
      // rule still flags `return voidCall()` and void in expressions. (302
      // findings otherwise, all of this shape.)
      '@typescript-eslint/no-confusing-void-expression': ['error', { ignoreArrowShorthand: true }],
      // JSX event props (`onClick={async () => ...}`) may be async: React
      // ignores the handler's return value (63 findings otherwise, all of
      // this shape). Such a handler must still catch its own errors.
      // Promises passed anywhere else that expects a void callback
      // (setTimeout, addEventListener, array methods) stay errors.
      '@typescript-eslint/no-misused-promises': ['error', { checksVoidReturn: { attributes: false } }],
      // `name?.trim() || username?.trim() || 'Signed in'`: on strings, `||`
      // deliberately treats '' as missing too, which `??` would not.
      '@typescript-eslint/prefer-nullish-coalescing': ['error', { ignorePrimitives: { string: true } }],
      // Fights no-non-null-assertion: it rewrites `x as T` to `x!`, which
      // the other rule bans (typescript-eslint's own docs: use one or the
      // other).
      '@typescript-eslint/non-nullable-type-assertion-style': 'off',
      // DEFERRED (off for now; follow-up): 44 findings in 19 non-test files
      // (measured 2026-10-07), three quarters of them `arr[i]!` /
      // `map.get(k)!` after a bounds or has() check, i.e. the
      // noUncheckedIndexedAccess fight the guide describes. Each needs a
      // real guard or a restructure, not a mechanical edit. Tests turn it
      // off for good, below.
      '@typescript-eslint/no-non-null-assertion': 'off',
      // DEFERRED (off for now; follow-up): 46 findings in 27 files
      // (measured 2026-10-07, tests included). Most are
      // `?.` / `??` guards on API response fields whose TypeScript types say
      // non-null while the server can still omit them; dropping a guard
      // changes runtime behaviour, so each site needs a decision (tighten
      // the type to optional, or drop the guard), not an autofix.
      '@typescript-eslint/no-unnecessary-condition': 'off',
    },
  },
  // Tests: the guide's test-only relaxations. vi.fn(async () => ...) mocks
  // an async interface with a synchronous body; mocked fetch and JSON
  // bodies are `any` that the test asserts on straight away; `x!` follows an
  // expect() that already proved x is there; no-op arrow stubs.
  {
    files: TEST_FILES,
    rules: {
      '@typescript-eslint/require-await': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      // Same reason as the three above: vi.mock factories forward to an
      // untyped vi.fn() (`usePathname: () => mockUsePathname()`), and
      // mock.calls entries are `any`.
      '@typescript-eslint/no-unsafe-return': 'off',
      '@typescript-eslint/no-unsafe-call': 'off',
      // `expect(obj.method).toHaveBeenCalled()` reads a method without
      // calling it; that is how Vitest spies are asserted on, and `this`
      // never matters there.
      '@typescript-eslint/unbound-method': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
      // Plus anonymous `function () {}`: stubs assigned onto globals
      // (window.PushManager) must be constructible, which arrows are not.
      '@typescript-eslint/no-empty-function': ['error', { allow: ['arrowFunctions', 'functions'] }],
      // fetch mocks' first argument is typed `RequestInfo | URL`, and the
      // tests stringify it to assert the URL. The code under test passes a
      // string or URL; a Request would fail the assertion anyway.
      '@typescript-eslint/no-base-to-string': 'off',
    },
  },

  // The same presets for the plain-JS scripts, each linted against the
  // tsconfig that type-checks it. JavaScript has no compiler of its own, so
  // these rules (floating promises, unsafe `any` flow, needless conditions)
  // catch what tsc's checkJs cannot.
  {
    files: SERVICE_WORKER_FILES,
    extends: [tseslint.configs.strictTypeChecked, tseslint.configs.stylisticTypeChecked],
    languageOptions: {
      parserOptions: { project: './tsconfig.sw.json', tsconfigRootDir: import.meta.dirname },
    },
  },
  {
    files: NODE_SCRIPT_FILES,
    extends: [tseslint.configs.strictTypeChecked, tseslint.configs.stylisticTypeChecked],
    languageOptions: {
      parserOptions: { project: './tsconfig.scripts.json', tsconfigRootDir: import.meta.dirname },
    },
  },
  {
    files: [...SERVICE_WORKER_FILES, ...NODE_SCRIPT_FILES],
    rules: {
      // The strict preset also bans numbers in template literals. A number
      // always stringifies predictably (unlike objects or null/undefined,
      // which stay banned), and the scripts' log lines interpolate counts.
      '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
    },
  },

  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    '.next/**',
    'out/**',
    'build/**',
    'next-env.d.ts',
    // This repo's own build/output paths:
    'coverage/**',
    'playwright-report/**',
    'test-results/**',
  ]),
]);

export default eslintConfig;
