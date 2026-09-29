/** A warning triangle. Replaces the literal "⚠" character, which some
 * platforms render as a full-colour emoji (ignoring the theme's text
 * colour entirely) and others as a thin text glyph. Same inline-SVG
 * pattern as `InfoIcon.tsx`; `currentColor`, so the caller picks the
 * semantic colour.
 *
 * Decorative on purpose: `aria-hidden` lives here, and whatever the icon
 * warns about must also be said in text (or in the enclosing element's
 * accessible name) -- never by this icon alone. */
export function WarningIcon({ size = 16 }: { size?: number }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      data-icon="warning"
    >
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  );
}
