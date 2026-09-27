import { describe, it, expect, vi, afterEach } from 'vitest';
import { GET, dynamic } from './route';

describe('app/healthz/route.ts', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('is rendered per request, never prerendered at build time', () => {
    expect(dynamic).toBe('force-dynamic');
  });

  it('answers 200 "ok" without calling the api (or anything else)', async () => {
    const fetchSpy = vi.fn(() => Promise.reject(new Error('api is down')));
    vi.stubGlobal('fetch', fetchSpy);

    const response = GET();

    expect(response.status).toBe(200);
    expect(await response.text()).toBe('ok');
    expect(response.headers.get('Cache-Control')).toBe('no-store');
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
