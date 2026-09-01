import { describe, it, expect } from 'vitest';
import { sanitizeDescription } from './sanitizeHtml';

describe('sanitizeDescription', () => {
  it('keeps safe HTML tags intact', () => {
    const result = sanitizeDescription('<p>Signal failure</p><br/><strong>at Woking</strong>');
    expect(result).toContain('<p>Signal failure</p>');
    expect(result).toContain('<strong>at Woking</strong>');
  });

  it('strips script tags and event handler attributes', () => {
    const result = sanitizeDescription('<p onclick="alert(1)">Safe text</p><script>alert(2)</script>');
    expect(result).not.toContain('<script>');
    expect(result).not.toContain('onclick');
    expect(result).toContain('Safe text');
  });

  it('forces target=_blank and rel=noopener on links', () => {
    const result = sanitizeDescription('<a href="https://example.com">More info</a>');
    expect(result).toContain('target="_blank"');
    expect(result).toContain('rel="noopener"');
  });
});
