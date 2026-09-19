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

/** review §3.5.9: an anchor whose visible text is byte-identical to its own
 * `href` ("https://www.nationalrail.co.uk/stations_destinations/passenger-
 * assist.aspx") reads as noise, not a destination. Rewritten to the
 * hostname plus an outbound arrow, with the full URL kept reachable via
 * `title` -- exactly the same swap `StationAccessibilitySection.tsx`'s own
 * `link`-kind nodes apply, via the same `new URL(...).hostname` shape, so a
 * raw URL reads the same way whether it arrived as a `link` node or as an
 * `<a>` inside sanitized rich text.
 *
 * Only fires on an EXACT match: an anchor whose author already wrote real
 * link text ("click here", a station name) is untouched. Anchors nested
 * inside another (impossible once DOMPurify has run) are not a concern
 * here. */
function rewriteRawUrlLinks(root: Element): void {
  const anchors = root.querySelectorAll('a[href]');
  anchors.forEach((anchor) => {
    const href = anchor.getAttribute('href')?.trim() ?? '';
    const text = anchor.textContent?.trim() ?? '';
    if (href === '' || text !== href) return;
    let hostname: string;
    try {
      hostname = new URL(href).hostname.replace(/^www\./i, '');
    } catch {
      return;
    }
    if (hostname === '') return;
    anchor.setAttribute('title', href);
    anchor.textContent = `${hostname} ↗`;
  });
}

/** UK landline/mobile numbers written as plain text in note copy --
 * `0345 077 4224`, `020 7946 0958`, `+44 20 7946 0958` -- linkified to
 * `tel:` so a phone open on the same page can dial them directly (review
 * §3.5.8). Matches a leading `0` or `+44` followed by 9-10 more digits
 * (spaces/hyphens/parens as separators, which every UK format the sample
 * uses employs), the shape both a 10-digit landline (`0345 077 4224`, 10
 * digits after the leading 0) and an 11-digit mobile (`07700 900123`) share.
 * Deliberately conservative -- a plausible-looking run of digits that isn't
 * really a phone number is far less costly to leave as plain text than a
 * false match turning an unrelated number into a dead `tel:` link. */
const UK_PHONE_PATTERN =
  /(?:\+44\s?\d{2,4}|\(?0\d{2,4}\)?)(?:[\s-]?\d){6,8}\b/g;

/** Digits and a leading `+` only, mirroring `stationAccessibility.ts`'s own
 * `digitsOnly` -- duplicated rather than imported, since importing that
 * module here would create a cycle (`stationAccessibility.ts` already
 * imports `sanitizeRichText` from this one). */
function digitsOnly(value: string): string {
  return value.replace(/[^\d+]/g, '');
}

/** Walks every text node NOT already inside an `<a>` (an anchor's own
 * visible text is never rewritten a second time here) and wraps each phone-
 * shaped run in a `tel:` anchor. Runs before `rewriteRawUrlLinks`, which
 * only looks at `<a>` elements and so never revisits what this just
 * created. Uses `document.createTreeWalker` off the fragment's own
 * `ownerDocument`, matching `demoteHeadings`'s SSR-safe pattern -- there is
 * no global `document` during Next's server render. */
function linkifyPhoneNumbers(root: Element): void {
  const doc = root.ownerDocument;
  const walker = doc.createTreeWalker(root, 1 /* NodeFilter.SHOW_ELEMENT */ | 4 /* SHOW_TEXT */);
  const textNodes: Text[] = [];
  let current = walker.nextNode();
  while (current) {
    if (current.nodeType === 3 && current.parentElement?.closest('a') === null) {
      textNodes.push(current as unknown as Text);
    }
    current = walker.nextNode();
  }
  for (const textNode of textNodes) {
    const text = textNode.textContent ?? '';
    UK_PHONE_PATTERN.lastIndex = 0;
    if (!UK_PHONE_PATTERN.test(text)) continue;
    UK_PHONE_PATTERN.lastIndex = 0;
    const fragment = doc.createDocumentFragment();
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = UK_PHONE_PATTERN.exec(text)) !== null) {
      if (match.index > lastIndex) {
        fragment.appendChild(doc.createTextNode(text.slice(lastIndex, match.index)));
      }
      const digits = digitsOnly(match[0]);
      if (digits.replace(/^\+/, '').length >= 10) {
        const anchor = doc.createElement('a');
        anchor.setAttribute('href', `tel:${digits}`);
        anchor.textContent = match[0];
        fragment.appendChild(anchor);
      } else {
        fragment.appendChild(doc.createTextNode(match[0]));
      }
      lastIndex = match.index + match[0].length;
    }
    if (lastIndex < text.length) fragment.appendChild(doc.createTextNode(text.slice(lastIndex)));
    textNode.replaceWith(fragment);
  }
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
  // Phone numbers first: a `tel:` anchor this creates must not then be
  // mistaken for a "raw URL as its own link text" case by the next pass
  // (it never would be -- `020 7946 0958` never equals `tel:02079460958`
  // -- but doing the text-node pass while there are still fewer anchors
  // around is simpler to reason about than the reverse order).
  linkifyPhoneNumbers(body);
  rewriteRawUrlLinks(body);
  return body.innerHTML;
}
