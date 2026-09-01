import DOMPurify from 'isomorphic-dompurify';

// Registered once at module load. `disruption.description` comes from the
// Darwin/Knowledgebase feed already fully HTML-entity-decoded by the time
// it reaches the frontend (see poller-incidents' quick_xml parsing) — it's
// real markup, not escaped/serialized XML needing re-parsing. DOMPurify's
// ALLOWED_ATTR strips `target`/`rel` by default since they're not in the
// allowlist below; this hook adds them back on every surviving `<a>` so
// external links don't inherit this page's window/referrer.
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
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
