import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: [['html', { outputFolder: 'e2e-report', open: 'never' }]],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:3000',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
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
  ],
  webServer: process.env.E2E_BASE_URL
    ? undefined
    : {
        command: 'npm run dev',
        url: 'http://localhost:3000',
        reuseExistingServer: !process.env.CI,
      },
});
