import { describe, it, expect } from 'vitest';
import { sanitizeDescription, sanitizeRichText } from './sanitizeHtml';

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

/** Station accessibility & facilities copy -- see
 * docs/superpowers/specs/2026-09-16-structured-accessibility-rendering-design.md
 * §2.4 and §4.7. 708 of the feed's 6,996 strings (10.1%) carry HTML; every
 * example below is a verbatim value from the 31-station survey unless it is
 * an attack string. */
describe('sanitizeRichText', () => {
  it('keeps the eight tags the feed actually uses', () => {
    const result = sanitizeRichText(
      '<p><strong>Meeting points</strong> <em>see below</em></p><ul><li>Platform <u>1</u></li></ul>',
    );
    expect(result).toContain('<strong>Meeting points</strong>');
    expect(result).toContain('<em>see below</em>');
    expect(result).toContain('<ul>');
    expect(result).toContain('<li>');
    expect(result).toContain('<u>1</u>');
  });

  // §8's "sanitizer tests" bullet, all three cases.
  it('renders a script, a javascript: href and an event handler inert', () => {
    const result = sanitizeRichText(
      '<p onclick="alert(1)">Safe</p><script>alert(2)</script>' +
        '<a href="javascript:alert(3)">Tap</a><img src=x onerror="alert(4)">',
    );
    expect(result).not.toMatch(/script/i);
    expect(result).not.toMatch(/onclick/i);
    expect(result).not.toMatch(/onerror/i);
    expect(result).not.toMatch(/javascript:/i);
    expect(result).not.toMatch(/<img/i);
    expect(result).toContain('Safe');
  });

  it('keeps https, http and mailto hrefs', () => {
    // `http:` is 45 of the sample's 193 anchors -- an allowlist without it
    // silently drops 23% of the feed's links.
    expect(sanitizeRichText('<a href="http://www.apcoa.co.uk">APCOA</a>')).toContain(
      'href="http://www.apcoa.co.uk"',
    );
    expect(sanitizeRichText('<a href="https://example.com">x</a>')).toContain('href="https://');
    expect(
      sanitizeRichText('<a href="mailto:customer.relations@scotrail.co.uk">email</a>'),
    ).toContain('href="mailto:');
    expect(sanitizeRichText('<a href="tel:03450774224">call</a>')).toContain('href="tel:');
  });

  it('drops an href whose scheme is not on the allowlist, keeping the link text', () => {
    const result = sanitizeRichText('<a href="data:text/html;base64,PHNjcmlwdD4=">Tap</a>');
    expect(result).not.toContain('data:');
    expect(result).toContain('Tap');
  });

  it('hardens every surviving anchor the way the incident sanitizer does', () => {
    const result = sanitizeRichText('<a href="https://example.com">More</a>');
    expect(result).toContain('target="_blank"');
    expect(result).toContain('rel="noopener"');
  });

  // §4.7: all nine `h2`s in the sample sit inside three notes at MAN. Left
  // alone they would be siblings of the section's own heading in the page
  // outline; demoted to `h4` they would skip a level and fail axe's
  // `heading-order`. Block-level bold says what they mean and stays out of
  // the outline entirely.
  it('demotes a heading to bold text rather than passing it through', () => {
    const result = sanitizeRichText('<h2>Passenger Assistance</h2><p>Details</p>');
    expect(result).not.toMatch(/<h[1-6]/i);
    expect(result).toContain('<p><strong>Passenger Assistance</strong></p>');
    expect(result).toContain('<p>Details</p>');
  });

  it('decodes the feed\'s entities instead of printing them literally', () => {
    expect(sanitizeRichText('<p>ramps can&#39;t be deployed</p>')).toContain("can't");
    expect(sanitizeRichText('<p>&quot;Step Free Access&quot;</p>')).toContain('"Step Free Access"');
  });

  it('escapes a bare ampersand rather than leaving it to be re-parsed', () => {
    expect(sanitizeRichText('<p>Fares & tickets</p>')).toContain('Fares &amp; tickets');
  });

  it('leaves an anchor whose scheme was rejected as inert text, not a hardened non-link', () => {
    const result = sanitizeRichText('<a href="javascript:alert(1)">Tap</a>');
    expect(result).not.toContain('href');
    expect(result).not.toContain('target=');
    expect(result).not.toContain('rel=');
    expect(result).toContain('Tap');
  });

  it('keeps the two allowlists separate: the incident one still rejects headings', () => {
    // `ul`/`li` have always been on `sanitizeDescription`'s list; headings
    // never were, and widening that one to suit this feature is exactly
    // what a second function avoids.
    const result = sanitizeDescription('<h2>Heading</h2><ul><li>One</li></ul>');
    expect(result).not.toMatch(/<h[1-6]/i);
    expect(result).toContain('Heading');
    expect(result).toContain('<ul><li>One</li></ul>');
  });
});
