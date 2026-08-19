/** The "ⓘ" affordance shared by the app's tooltip triggers.
 *
 * `@tabler/icons-react` isn't a project dependency (checked package.json)
 * — inline SVG instead of the literal "ⓘ" character, which renders as a
 * broken-looking glyph on an emoji/font fallback rather than a
 * recognisable info symbol.
 *
 * Decorative on purpose: `aria-hidden` lives here and the accessible name
 * belongs on the `ActionIcon` wrapping it, which differs per trigger. */
export function InfoIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="16" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12.01" y2="8" />
    </svg>
  );
}
