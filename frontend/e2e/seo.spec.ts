import { test, expect } from '@playwright/test';

// SEO phase 1 against the real app: the sitemap, a station page's canonical
// URL and structured data, the case redirect for station codes, and the
// live link-preview images.
const CRS = process.env.E2E_REAL_STATION_CRS ?? 'KGX';
const INCIDENT_ID = process.env.E2E_REAL_INCIDENT_ID ?? 'E879EB6C791C470AB6C2A7458AE68C3B';
const LINE_ID = process.env.E2E_REAL_LINE_ID ?? 'lner-ecml';

test('/sitemap.xml lists public pages and none of the short-lived ones', async ({ request }) => {
  const response = await request.get('/sitemap.xml');
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toMatch(/xml/);
  const body = await response.text();
  expect(body).toContain('<urlset');
  expect(body).toMatch(new RegExp(`<loc>[^<]*/stations/${CRS}</loc>`));
  expect(body).toMatch(/<loc>[^<]*\/lines<\/loc>/);
  expect(body).toMatch(/<loc>[^<]*\/trains<\/loc>/);
  expect(body).not.toMatch(/<loc>[^<]*\/trains\?/);
  expect(body).not.toMatch(/<loc>[^<]*\/train\//);
  expect(body).not.toMatch(/<loc>[^<]*\/incidents\/[^<]/);
  expect(body).not.toMatch(/<loc>[^<]*\/history/);
});

test('the Trains section names the bare /trains as its canonical URL, whatever the search', async ({ page }) => {
  await page.goto('/trains?station=ZFA&date=2099-01-05');
  await expect(page).toHaveTitle('Trains · Distant Signal');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', /\/trains$/);
  await expect(page.locator('meta[name="description"]')).toHaveAttribute(
    'content',
    'Timetables for every station: departures, arrivals, trains between two stations, or one train by headcode or UID.',
  );
});

test('a lower-case station code redirects permanently to the upper-case page', async ({ request }) => {
  const response = await request.get(`/stations/${CRS.toLowerCase()}`, { maxRedirects: 0 });
  expect(response.status()).toBe(308);
  expect(response.headers().location).toMatch(new RegExp(`/stations/${CRS}$`));
});

// Live link previews (lib/livePreview.ts): each page lists its own card,
// versioned by its state, and the card is served as a cacheable PNG.
const LIVE_PREVIEWS: [page: string, missing: string, description: RegExp][] = [
  [`/incidents/${INCIDENT_ID}`, '/incidents/NO-SUCH-INCIDENT', /^(Disruption|Planned work)/],
  [`/lines/${LINE_ID}`, '/lines/no-such-line', /^Live status, disruption/],
  [`/stations/${CRS}`, '/stations/ZZZ', /^Disruption, accessibility and departures for /],
];

for (const [path, missing, description] of LIVE_PREVIEWS) {
  test(`${path} lists its own live preview image, served and cacheable`, async ({ page, request }) => {
    await page.goto(path);
    const url = await page.locator('meta[property="og:image"]').getAttribute('content');
    expect(url).toMatch(new RegExp(`${path}/card\\.png\\?v=[0-9a-f]{8}$`));
    await expect(page.locator('meta[property="og:image:alt"]')).toHaveAttribute('content', /on Distant Signal$/);
    await expect(page.locator('meta[name="twitter:image"]')).toHaveAttribute('content', url!);
    await expect(page.locator('meta[property="og:description"]')).toHaveAttribute('content', description);

    const response = await request.get(new URL(url!).pathname + new URL(url!).search);
    expect(response.status()).toBe(200);
    expect(response.headers()['content-type']).toBe('image/png');
    expect(response.headers()['cache-control']).toMatch(/public/);
    const png = await response.body();
    expect(png.subarray(1, 4).toString('ascii')).toBe('PNG');
    expect([png.readUInt32BE(16), png.readUInt32BE(20)]).toEqual([1200, 630]);

    expect((await request.get(`${missing}/card.png`)).status()).toBe(404);
  });
}

test('a station page names its canonical URL and describes itself as structured data', async ({ page }) => {
  await page.goto(`/stations/${CRS}?utm_source=x`);
  await expect(page.locator('html')).toHaveAttribute('lang', 'en-GB');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', new RegExp(`/stations/${CRS}$`));
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', new RegExp(`/stations/${CRS}$`));
  const jsonLd = await page.locator('script[type="application/ld+json"]').first().textContent();
  const data = JSON.parse(jsonLd ?? '[]') as { '@type': string; identifier?: string }[];
  expect(data[0]).toMatchObject({ '@type': 'TrainStation', identifier: CRS });
  expect(data[1]?.['@type']).toBe('BreadcrumbList');
});
