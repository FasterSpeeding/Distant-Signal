import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'node:path';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    setupFiles: ['./vitest.setup.ts'],
    globals: true,
    // Vitest's own default `include` glob (`**/*.{test,spec}.?(c|m)[jt]s?(x)`)
    // also matches `*.spec.ts` -- which, until frontend/e2e/service-worker.spec.ts
    // (docs/superpowers/plans/2026-09-02-pwa-service-worker.md, Task 7), this
    // repo's empty-but-configured Playwright suite never actually used. That
    // file imports `test`/`expect` from `@playwright/test`, not Vitest's own,
    // so Vitest picking it up fails immediately with "did not expect
    // test.describe() to be called here." Restricting `include` to this
    // repo's actual, already-established convention (colocated `*.test.ts(x)`
    // files only -- see this plan's Global Constraints) keeps Vitest out of
    // `frontend/e2e/` entirely, which is Playwright's domain
    // (playwright.config.ts's own `testDir`).
    include: ['**/*.test.{ts,tsx,js,jsx}'],
    // R-093 / FE-13: Vitest's 5 s default is per test, wall clock. The jsdom
    // component tests that drive multi-step flows with user-event
    // (PlanTripFlow, TrainSearchForm, StationAccessibilitySection on real
    // payloads, ...) take 1-3 s on an idle machine, and on a busy shared host
    // (load average ~50, 2026-10-01) 15 of 3,146 tests crossed 5 s, so
    // `npm test` exited 1 with nothing wrong. 20 s absorbs that contention;
    // a genuinely hung test (an await that never settles) still fails, just
    // 15 s later.
    testTimeout: 20_000,
  },
  resolve: {
    alias: { '@': path.resolve(__dirname, '.') },
  },
});
