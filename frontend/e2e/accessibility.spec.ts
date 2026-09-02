import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Reproduces the accessibility audit's own method (`axe.run` per page,
// documented in docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md)
// as a committed, repeatable test rather than an ad-hoc `browser_evaluate`
// injection. Like every other spec in this directory (see e2e/chat.spec.ts's
// own comment), this drives the REAL app through `playwright.config.ts`'s
// `webServer` (a real `next dev`) or a real deployed target
// (`E2E_BASE_URL`) -- it does not stand up its own backend, so every route
// here needs a real, reachable `api` service to render anything beyond a
// skeleton/error state.
//
// Two honest limits, carried over from the audit itself:
// 1. This only sees whatever severities/states are actually live at run
//    time -- it can't force a Planned Closure or an informational status
//    to exist just to check its badge. That's exactly why the deterministic
//    palette assertions in app/globals.test.ts (Task 4 Step 5) are the
//    primary net for filled-surface contrast, and this is the secondary,
//    real-DOM one.
// 2. The real line/incident/station identifiers below (`gwr-main-line`,
//    `E879EB6C791C470AB6C2A7458AE68C3B`, `PAD`) are the same ones the audit
//    used against its own live deployment -- override them via the env
//    vars below if a different `E2E_BASE_URL` deployment doesn't have this
//    exact data.
const REAL_LINE_ID = process.env.E2E_REAL_LINE_ID ?? 'gwr-main-line';
const REAL_INCIDENT_ID = process.env.E2E_REAL_INCIDENT_ID ?? 'E879EB6C791C470AB6C2A7458AE68C3B';
const REAL_STATION_CRS = process.env.E2E_REAL_STATION_CRS ?? 'PAD';

// The five rule IDs this plan's tasks fix. Not axe's full default ruleset --
// a broader failure (e.g. a rule this plan never touched) shouldn't fail
// this spec and get miscategorized as a regression in this work.
const RULES_UNDER_TEST = ['color-contrast', 'landmark-one-main', 'region', 'heading-order', 'page-has-heading-one'];

async function expectNoViolations(page: import('@playwright/test').Page) {
  const results = await new AxeBuilder({ page }).withRules(RULES_UNDER_TEST).analyze();
  expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
}

test.describe('accessibility: anonymous-reachable routes', () => {
  test('/', async ({ page }) => {
    await page.goto('/');
    await expectNoViolations(page);
  });

  test('/lines', async ({ page }) => {
    await page.goto('/lines');
    await expectNoViolations(page);
  });

  test(`/lines/${REAL_LINE_ID}`, async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}`);
    await expectNoViolations(page);
  });

  test(`/lines/${REAL_LINE_ID}/history, Timeline tab`, async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}/history`);
    await expectNoViolations(page);
  });

  test(`/lines/${REAL_LINE_ID}/history, Trends tab`, async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}/history`);
    await page.getByRole('tab', { name: 'Trends' }).click();
    await expectNoViolations(page);
  });

  test('/stations', async ({ page }) => {
    await page.goto('/stations');
    await expectNoViolations(page);
  });

  test(`/stations/${REAL_STATION_CRS}`, async ({ page }) => {
    await page.goto(`/stations/${REAL_STATION_CRS}`);
    await expectNoViolations(page);
  });

  test(`/incidents/${REAL_INCIDENT_ID}`, async ({ page }) => {
    await page.goto(`/incidents/${REAL_INCIDENT_ID}`);
    await expectNoViolations(page);
  });

  // Deliberate 404, exercising Task 3's not-found.tsx fix + Task 1's
  // <main> landmark (which every not-found.tsx inherits for free -- see
  // this plan's Decision 1).
  test('a deliberate 404 (/lines/nonexistent-line-slug)', async ({ page }) => {
    await page.goto('/lines/nonexistent-line-slug');
    await expectNoViolations(page);
  });
});
