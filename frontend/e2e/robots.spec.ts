import { test, expect } from '@playwright/test';

test('/robots.txt is served as text/plain with the expected rules', async ({ request }) => {
  const response = await request.get('/robots.txt');
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toMatch(/^text\/plain/);

  const body = await response.text();
  expect(body).toContain('User-Agent: *');
  expect(body).toContain('Disallow: /api/');
  expect(body).toContain('Disallow: /journeys/');
  expect(body).toContain('Disallow: /groups');
  expect(body).toContain('User-Agent: GPTBot');
  expect(body).not.toContain('undefined');
});
