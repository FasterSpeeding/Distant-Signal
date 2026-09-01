import { describe, it, expect } from 'vitest';
import manifest from './manifest';

describe('manifest', () => {
  it('names the app "Distant Signal" for both name and short_name, unabbreviated', () => {
    const result = manifest();
    expect(result.name).toBe('Distant Signal');
    expect(result.short_name).toBe('Distant Signal');
  });

  it('uses the trimmed description, distinct from layout.tsx metadata.description', () => {
    expect(manifest().description).toBe(
      'A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.',
    );
  });

  it('starts at the site root', () => {
    expect(manifest().start_url).toBe('/');
  });

  it("renders standalone with the app's light background and grape-6 brand theme colour", () => {
    const result = manifest();
    expect(result.display).toBe('standalone');
    expect(result.background_color).toBe('#ffffff');
    expect(result.theme_color).toBe('#be4bdb');
  });

  it('declares exactly the two required icons, 192x192 and 512x512, image/png, no purpose field', () => {
    expect(manifest().icons).toEqual([
      { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
    ]);
  });
});
