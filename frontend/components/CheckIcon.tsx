/** A tick, for a "done" state such as "Copied!" on a copy/share button.
 * Same reasoning as `InfoIcon.tsx`: inline SVG rather than the literal "✓"
 * character, whose rendering depends on the font stack.
 *
 * Decorative on purpose: `aria-hidden` lives here and the accessible name
 * belongs on the button wrapping it. */
export function CheckIcon() {
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
      data-icon="check"
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}
