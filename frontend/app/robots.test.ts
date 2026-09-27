import { describe, it, expect, afterEach } from 'vitest';
import robots, { dynamic } from './robots';

describe('app/robots.ts', () => {
  afterEach(() => {
    delete process.env.NEXT_PUBLIC_SITE_URL;
  });

  it('renders per request, so the runtime NEXT_PUBLIC_SITE_URL is used rather than a build-time value', () => {
    expect(dynamic).toBe('force-dynamic');
  });

  it('reads NEXT_PUBLIC_SITE_URL at call time, trimming a trailing slash', () => {
    process.env.NEXT_PUBLIC_SITE_URL = 'https://ds.example/';
    expect(robots().host).toBe('https://ds.example');
  });

  it('omits host and sitemap when NEXT_PUBLIC_SITE_URL is unset', () => {
    const result = robots();
    expect(result.host).toBeUndefined();
    expect(result.sitemap).toBeUndefined();
  });
});
