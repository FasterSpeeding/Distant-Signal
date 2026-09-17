import DOMPurify from 'isomorphic-dompurify';

// Registered once at module load. `disruption.description` comes from the
// Darwin/Knowledgebase feed already fully HTML-entity-decoded by the time
// it reaches the frontend (see poller-incidents' quick_xml parsing) — it's
// real markup, not escaped/serialized XML needing re-parsing. DOMPurify's
// ALLOWED_ATTR strips `target`/`rel` by default since they're not in the
// allowlist below; this hook adds them back on every surviving `<a>` so
// external links don't inherit this page's window/referrer.
//
// Only on an anchor that still HAS an href. An `<a>` whose href the
// sanitizer rejected (a `javascript:` URL, or any scheme outside
// `sanitizeRichText`'s allowlist) survives as an inert wrapper around its
// own text, and putting `target`/`rel` on that would be link hardening
// applied to something that is no longer a link.
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A' && node.hasAttribute('href')) {
    node.setAttribute('target', '_blank');
    node.setAttribute('rel', 'noopener');
  }
});

const ALLOWED_TAGS = ['p', 'br', 'strong', 'b', 'em', 'i', 'ul', 'ol', 'li', 'a'];
const ALLOWED_ATTR = ['href'];

/** The single sanitizer for every incident/disruption description this app
 * renders as HTML — shared by `DisruptionDetail.tsx` (a line's/station's
 * inline issue list) and `app/incidents/[id]/page.tsx` (the incident's own
 * detail page), so both apply the exact same allowlist and the same
 * forced `target="_blank" rel="noopener"` link hardening. Extracted out of
 * `DisruptionDetail.tsx`, where this previously lived file-local — see
 * docs/superpowers/specs/2026-08-31-incident-detail-page-design.md
 * Decision 5. */
export function sanitizeDescription(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR });
}

/** The tag inventory the station-accessibility survey actually found —
 * `p`, `a[href]`, `li`, `strong`, `ul`, `em`, `h2`, `u`, and nothing else
 * (docs/superpowers/specs/2026-09-16-structured-accessibility-rendering-design.md
 * §2.4) — plus `br` and `ol`, which do not occur today but are innocuous
 * and whose absence would silently destroy formatting the day the feed
 * starts using them (§4.7). `b`/`i` ride along for the same reason.
 *
 * `h1`-`h6` are admitted here only so `demoteHeadings` below can rewrite
 * them: a tag left out of the allowlist is dropped but its text kept
 * (DOMPurify's `KEEP_CONTENT` default), which would silently flatten a
 * note's emphasis rather than preserve it. */
const RICH_TEXT_TAGS = [
  'p',
  'br',
  'strong',
  'b',
  'em',
  'i',
  'u',
  'ul',
  'ol',
  'li',
  'a',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
];

const RICH_TEXT_ATTR = ['href'];

/** The three schemes the 193 surveyed anchors actually use (`https:` ×143,
 * `http:` ×45, `mailto:` ×5), plus `tel:` -- harmless, and the feed puts
 * assistance phone numbers in this copy as plain text today, so it is the
 * obvious next scheme to appear. `http:` is NOT optional: dropping it
 * would silently kill 23% of the sample's links. Everything else --
 * `javascript:`, `data:`, relative and protocol-relative URLs -- is
 * rejected, which is stricter than DOMPurify's own default. */
const RICH_TEXT_URI_REGEXP = /^(?:https?|mailto|tel):/i;

/** Rewrites every `h1`-`h6` in a sanitized fragment to `<p><strong>`.
 *
 * The page outline is `h1` (the station name, `app/stations/[crs]/page.tsx`)
 * then `h2` (`StationAccessibilitySection`'s own title), so a heading
 * arriving inside third-party note copy has nowhere safe to land: passing
 * an `h2` through makes a note a sibling of the section itself, and
 * demoting to `h4` -- the design's other suggestion -- would skip `h3` and
 * fail axe's `heading-order`. All nine `h2`s in the survey are emphasis,
 * not structure (three notes at `MAN`), so block-level bold says what they
 * mean without touching the outline at all.
 *
 * Operates on the sanitizer's own DOM output rather than on a serialized
 * string: a regex over markup is exactly what the design (§4.7) rules out,
 * and `ownerDocument` keeps this working under Node's SSR pass, where
 * there is no global `document`. */
function demoteHeadings(root: Element): void {
  const headings = root.querySelectorAll('h1, h2, h3, h4, h5, h6');
  headings.forEach((heading) => {
    const doc = heading.ownerDocument;
    const paragraph = doc.createElement('p');
    const strong = doc.createElement('strong');
    while (heading.firstChild) strong.appendChild(heading.firstChild);
    paragraph.appendChild(strong);
    heading.replaceWith(paragraph);
  });
}

/** The sanitizer for station accessibility & facilities copy -- the
 * `notes`/`location`/`note`/`*Notes`/`operatorName` fields where 708 of the
 * feed's 6,996 strings (10.1%) carry real HTML, currently printed as
 * literal tag soup.
 *
 * Deliberately a second allowlist rather than a widening of
 * `sanitizeDescription`'s: that one describes Darwin/Knowledgebase incident
 * copy and has its own evidence behind it, and quietly admitting list and
 * heading markup there to suit this feature would be a change nobody asked
 * for. Both share this module's DOMPurify instance, so both get the same
 * `target="_blank" rel="noopener"` hardening on every surviving anchor. */
export function sanitizeRichText(html: string): string {
  const body = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: RICH_TEXT_TAGS,
    ALLOWED_ATTR: RICH_TEXT_ATTR,
    ALLOWED_URI_REGEXP: RICH_TEXT_URI_REGEXP,
    RETURN_DOM: true,
  }) as unknown as Element;
  demoteHeadings(body);
  return body.innerHTML;
}
