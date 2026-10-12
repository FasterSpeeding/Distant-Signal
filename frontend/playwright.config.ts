import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  retries: process.env.CI ? 2 : 0,
  reporter: [['html', { outputFolder: 'e2e-report', open: 'never' }]],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:3000',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      testIgnore: /visual\.spec\.ts/,
      use: { ...devices['Desktop Chrome'] },
    },
    // Gecko only, and only for the nav spec. Nav layout is the one thing
    // in this suite whose correctness is font-metric dependent: the same
    // bar markup measured 1112px in Chromium and 1079px in Firefox inside
    // the same 1100px container, so it wrapped in one engine and not the
    // other (see e2e/nav.spec.ts's own header for the full set of
    // measurements). A Chromium-only check would have called that fixed
    // or broken depending purely on which browser it ran in.
    //
    // Scoped with `testMatch` rather than added as a second full project
    // on purpose: everything else here (the axe sweep especially) is
    // engine-independent and slow, and doubling the suite to re-assert it
    // would buy nothing.
    {
      name: 'firefox-nav',
      testMatch: /nav\.spec\.ts/,
      use: { ...devices['Desktop Firefox'] },
    },
    // Visual regression screenshots (e2e/visual.spec.ts). Run on its own
    // with `--project=visual`, and only inside the pinned Playwright
    // container that scripts/visual-tests.py starts: the baselines are
    // only comparable when the fonts and renderer are the same, which is
    // why the snapshot paths below carry no platform suffix. See
    // docs/style-guide.md, "Visual regression".
    {
      name: 'visual',
      testMatch: /visual\.spec\.ts/,
      // One worker: the pages share one api and `next start`, and a
      // contended server can return a page before a slow fetch finishes.
      fullyParallel: false,
      snapshotPathTemplate: '{testDir}/visual-baselines/{arg}{ext}',
      use: { ...devices['Desktop Chrome'], serviceWorkers: 'block' },
      expect: {
        // A full-page capture of the longest pages (/lines at 1366px) takes
        // a few seconds per attempt, and Playwright takes two to check the
        // page has settled; the default 5s isn't enough.
        timeout: 30_000,
        toHaveScreenshot: {
          // `threshold` is the per-pixel colour tolerance (0 to 1, YIQ):
          // 0.2 is Playwright's default and absorbs anti-aliasing noise.
          // `maxDiffPixelRatio` 0.01 lets up to 1% of pixels differ, which
          // a one-off glyph or a shifted badge exceeds, so a real layout or
          // colour change still fails.
          threshold: 0.2,
          maxDiffPixelRatio: 0.01,
        },
      },
    },
  ],
  ...(process.env.E2E_BASE_URL
    ? {}
    : {
        webServer: {
          command: 'npm run dev',
          url: 'http://localhost:3000',
          reuseExistingServer: !process.env.CI,
        },
      }),
});
