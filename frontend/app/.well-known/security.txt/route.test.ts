import { describe, it, expect, afterEach, vi } from 'vitest';
import * as legal from '@/lib/legal';
import { GET, buildSecurityTxt } from './route';

vi.mock('@/lib/legal', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/legal')>();
  return { ...actual, legalPagesPublished: vi.fn(() => false) };
});

// LEG-2
describe('/.well-known/security.txt', () => {
  afterEach(() => {
    vi.mocked(legal.legalPagesPublished).mockReturnValue(false);
  });

  it('404s until the legal pages are published, so no placeholder address is advertised', async () => {
    const res = GET();
    expect(res.status).toBe(404);
    expect(await res.text()).not.toContain('[[');
  });

  it('serves RFC 9116 fields as text/plain once published', async () => {
    vi.mocked(legal.legalPagesPublished).mockReturnValue(true);
    const res = GET();
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toBe('text/plain; charset=utf-8');
    const body = await res.text();
    expect(body).toMatch(/^Contact: mailto:/m);
    expect(body).toMatch(/^Expires: \d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/m);
  });

  it('expires 180 days out and names the canonical URL and policy when the site origin is known', () => {
    const body = buildSecurityTxt(new Date('2026-09-27T12:00:00.123Z'), 'https://ds.example');
    expect(body).toBe(
      [
        `Contact: mailto:${legal.LEGAL_CONFIG.CONTACT_EMAIL}`,
        'Expires: 2027-03-26T12:00:00Z',
        'Canonical: https://ds.example/.well-known/security.txt',
        'Policy: https://ds.example/contact',
        'Preferred-Languages: en',
        '',
      ].join('\n'),
    );
  });

  it('omits Canonical and Policy when the site origin is not configured', () => {
    const body = buildSecurityTxt(new Date('2026-09-27T00:00:00Z'), undefined);
    expect(body).not.toMatch(/Canonical|Policy/);
  });
});
