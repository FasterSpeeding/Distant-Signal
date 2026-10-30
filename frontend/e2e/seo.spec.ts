import { test, expect } from '@playwright/test';

// SEO phase 1 against the real app: the sitemap, a station page's canonical
// URL and structured data, and the case redirect for station codes.
const CRS = process.env.E2E_REAL_STATION_CRS ?? 'KGX';

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
