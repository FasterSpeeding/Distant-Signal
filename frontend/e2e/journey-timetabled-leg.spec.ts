import { test, expect, type BrowserContext, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Timetabled legs (DS-237), against the seeded backend
// (e2e/fixtures/seed.sql): the seeded Metrolink feed's Blue Line runs
// Piccadilly 07:02, Deansgate-Castlefield 07:06, Eccles 07:27 today, with a
// disruption on the line. The session (fixture-owner) saves a journey with
// that tram ride, as a plan's tram leg is saved (its network, line and
// headsign), and one three days on when the timetable has no service; the
// journey page then shows each as scheduled, with its stops, the
// disruption and the note. The journey is deleted again at the end, so no
// other spec or screenshot sees it.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

async function logIn(context: BrowserContext) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  await context.addCookies([
    {
      name: 'distant_signal_session',
      value: SESSION_COOKIE!,
      domain: base.hostname,
      path: '/',
      expires: -1,
      httpOnly: true,
      secure: base.protocol === 'https:',
      sameSite: 'Lax',
    },
  ]);
}

/** `HH:MM` London time `days` from London's today, as an instant. */
function londonInstant(days: number, time: string): string {
  const today = new Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/London' }).format(new Date());
  const [year = 0, month = 1, day = 1] = today.split('-').map(Number);
  const [hour = 0, minute = 0] = time.split(':').map(Number);
  // Guess UTC, then correct by London's offset at that moment.
  const guess = new Date(Date.UTC(year, month - 1, day + days, hour, minute));
  const offset = new Intl.DateTimeFormat('en-GB', { timeZone: 'Europe/London', timeZoneName: 'shortOffset' })
    .formatToParts(guess)
    .find((part) => part.type === 'timeZoneName')?.value;
  const hours = offset && offset !== 'GMT' ? Number(offset.replace('GMT', '')) : 0;
  return new Date(guess.getTime() - hours * 3_600_000).toISOString();
}

function tramLeg(days: number) {
  return {
    mode: 'untracked',
    transitMode: 'tram',
    lineName: 'Blue Line',
    originStopId: '9400ZZMAPIC',
    destinationStopId: '9400ZZMAECC',
    plannedDeparture: londonInstant(days, '07:02'),
    plannedArrival: londonInstant(days, '07:27'),
    transitNetwork: 'metrolink',
    transitLineId: 'blue',
    headsign: 'Eccles',
  };
}

async function api(page: Page, method: string, path: string, body?: unknown) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const init: RequestInit =
        body === undefined
          ? { method }
          : { method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) };
      const response = await fetch(`/api${path}`, init);
      const text = await response.text();
      return { status: response.status, body: text ? JSON.parse(text) : null };
    },
    { method, path, body },
  );
}

test.describe('timetabled legs', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  let journeyId: number | null = null;

  test.afterAll(async ({ browser }) => {
    if (journeyId === null) return;
    const context = await browser.newContext();
    await logIn(context);
    const page = await context.newPage();
    await page.goto('/journeys');
    await api(page, 'DELETE', `/Journeys/${journeyId}`);
    await context.close();
  });

  test('a saved tram ride shows its timetabled stops, scheduled, never live', async ({ page, context }) => {
    await logIn(context);
    await page.goto('/journeys');
    const created = await api(page, 'POST', '/Journeys', { leg: tramLeg(0) });
    expect(created.status, JSON.stringify(created.body)).toBe(200);
    journeyId = created.body.journeyId as number;
    const added = await api(page, 'POST', `/Journeys/${journeyId}/legs`, tramLeg(3));
    expect(added.status, JSON.stringify(added.body)).toBe(200);

    await page.goto(`/journeys/${journeyId}`);
    const legs = page.locator('[data-untracked-leg]');
    await expect(legs).toHaveCount(2);
    const today = legs.nth(0);
    await expect(today.getByText('Scheduled', { exact: true })).toBeVisible();
    await expect(today.getByRole('link', { name: 'Blue Line' })).toHaveAttribute(
      'href',
      '/transit/metrolink/lines/blue',
    );
    await expect(today.getByText('Towards Eccles')).toBeVisible();
    const calls = today.locator('[data-timetabled-call]');
    await expect(calls).toHaveCount(3);
    await expect(calls.nth(0)).toContainText('07:02');
    await expect(calls.nth(0)).toContainText('Piccadilly');
    await expect(calls.nth(1)).toContainText('07:06');
    await expect(calls.nth(1)).toContainText('Deansgate-Castlefield');
    await expect(calls.nth(2)).toContainText('07:27');
    await expect(calls.nth(2)).toContainText('Eccles');
    await expect(today.getByText(/^Times from Metrolink's timetable, not live\./)).toBeVisible();
    // Nothing that reads as live.
    await expect(today.getByText(/on time|expected|\blate\b/i)).toHaveCount(0);

    // Three days on the seeded timetable has no service: a note, not a
    // break, and the line's disruption with its page.
    const later = legs.nth(1);
    await expect(
      later.getByText(
        'The timetable has no Blue Line trams between these stops that day. It may be closed for planned work, so check before you travel.',
      ),
    ).toBeVisible();
    await expect(later.locator('[data-timetabled-call]')).toHaveCount(0);
    const notice = later.locator('[data-timetabled-leg-notice]').first();
    await expect(notice).toContainText('Disruption reported on the Blue Line');
    await expect(notice.getByRole('link', { name: 'More about this disruption' })).toHaveAttribute(
      'href',
      /^\/transit\/metrolink\/incidents\//,
    );

    const results = await new AxeBuilder({ page }).include('[data-timetabled-leg]').analyze();
    expect(results.violations).toEqual([]);
  });
});
