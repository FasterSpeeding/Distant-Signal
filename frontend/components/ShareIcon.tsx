/** The "share" affordance (an arrow leaving a tray), for share/copy-link
 * buttons. Replaces the literal "⇪" character -- really the Caps Lock
 * symbol -- which falls back to an unrelated-looking glyph on many font
 * stacks. Same inline-SVG pattern as `InfoIcon.tsx`.
 *
 * Decorative on purpose: `aria-hidden` lives here and the accessible name
 * belongs on the button wrapping it. */
export function ShareIcon() {
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
      data-icon="share"
    >
      <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
      <polyline points="16 6 12 2 8 6" />
      <line x1="12" y1="2" x2="12" y2="15" />
    </svg>
  );
}
