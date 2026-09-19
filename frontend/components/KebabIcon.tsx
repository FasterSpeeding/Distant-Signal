/** The vertical "⋮" overflow-menu affordance, matching `InfoIcon.tsx`'s
 * own reasoning: `@tabler/icons-react` isn't a project dependency (checked
 * package.json) -- inline SVG instead of the literal "⋮" character, which
 * renders as a thin, easy-to-miss glyph on some font stacks rather than a
 * recognisable "more actions" symbol.
 *
 * Decorative on purpose: `aria-hidden` lives here and the accessible name
 * belongs on the `ActionIcon`/button wrapping it, which differs per
 * trigger (Task 3.6.7's `/track/mine` row menu is the first caller). */
export function KebabIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
    >
      <circle cx="12" cy="5" r="2" />
      <circle cx="12" cy="12" r="2" />
      <circle cx="12" cy="19" r="2" />
    </svg>
  );
}
