import { describe, it, expect } from 'vitest';
import manifest from './manifest';

describe('manifest', () => {
  it('names the app "Distant Signal", with a short name that fits under a home-screen icon', () => {
    const result = manifest();
    expect(result.name).toBe('Distant Signal');
    expect(result.short_name).toBe('DS Rail');
    expect(result.short_name!.length).toBeLessThanOrEqual(12);
  });

  it('uses the site description, the same as layout.tsx', () => {
    expect(manifest().description).toBe('Live UK rail status, train tracking and Delay Repay help.');
  });

  it('starts at the site root', () => {
    expect(manifest().start_url).toBe('/');
  });

  it("renders standalone with the light scheme's background, and the viewport's light theme colour", () => {
    const result = manifest();
    expect(result.display).toBe('standalone');
    expect(result.background_color).toBe('#ffffff');
    expect(result.theme_color).toBe('#ffffff');
  });

  it('declares the 192 and 512 icons plus a maskable 512', () => {
    expect(manifest().icons).toEqual([
      { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
      { src: '/icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
    ]);
  });
});
