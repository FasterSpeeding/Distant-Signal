import { describe, expect, it } from 'vitest';
import {
  LIVE_PREVIEW_CACHE_CONTROL,
  LIVE_PREVIEW_REVALIDATE_SECONDS,
  listPhrase,
  livePreviewImage,
  plainText,
  previewDigest,
  previewSpan,
  previewWhen,
} from './livePreview';

describe('the live preview cache', () => {
  it('is public, a minute fresh, then stale while one request refreshes it', () => {
    expect(LIVE_PREVIEW_REVALIDATE_SECONDS).toBe(60);
    expect(LIVE_PREVIEW_CACHE_CONTROL).toBe('public, max-age=60, s-maxage=60, stale-while-revalidate=300');
  });
});

describe('previewDigest and livePreviewImage', () => {
  it('is a stable 8-digit hex digest that changes with its input', () => {
    expect(previewDigest(['a', 1])).toMatch(/^[0-9a-f]{8}$/);
    expect(previewDigest(['a', 1])).toBe(previewDigest(['a', 1]));
    expect(previewDigest(['a', 2])).not.toBe(previewDigest(['a', 1]));
  });

  it('puts the digest on the URL as ?v=', () => {
    expect(livePreviewImage('/x/card.png', 'abcd1234', 'Alt')).toEqual({
      url: '/x/card.png?v=abcd1234',
      alt: 'Alt',
      width: 1200,
      height: 630,
      type: 'image/png',
    });
  });
});

describe('text helpers', () => {
  it('joins lists in plain English', () => {
    expect(listPhrase([])).toBe('');
    expect(listPhrase(['A'])).toBe('A');
    expect(listPhrase(['A', 'B'])).toBe('A and B');
    expect(listPhrase(['A', 'B', 'C'])).toBe('A, B and C');
    expect(listPhrase(['A', 'B', 'C', 'D'])).toBe('A, B, C and 1 more');
  });

  it('turns status HTML into plain text', () => {
    expect(plainText('<p>Fault at  Rainham &amp; Gillingham&nbsp;</p><p>Allow &#39;extra&#39; time</p>')).toBe(
      "Fault at Rainham & Gillingham Allow 'extra' time",
    );
  });

  it('writes times in UK time, with the day', () => {
    // 1 Jul is in BST: 12:20Z is 13:20 in the UK.
    expect(previewWhen('2026-07-01T12:20:00Z')).toBe('Wed 1 Jul, 13:20');
    expect(previewSpan('2026-07-01T12:20:00Z', '2026-07-01T12:56:00Z')).toBe('Wed 1 Jul, 13:20 to 13:56');
    expect(previewSpan('2026-07-01T12:20:00Z', '2026-07-02T08:00:00Z')).toBe('Wed 1 Jul, 13:20 to Thu 2 Jul, 09:00');
  });
});
