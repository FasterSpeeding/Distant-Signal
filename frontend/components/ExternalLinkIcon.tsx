import { VisuallyHidden } from '@mantine/core';

/** The trailing "opens elsewhere" marker for a link with `target="_blank"`.
 * Replaces a literal "↗" in the link text, which a screen reader reads out
 * as "north east arrow" while never saying the one thing the arrow means:
 * that the link opens a new tab. The SVG is `aria-hidden`; the
 * `VisuallyHidden` text carries that meaning into the link's accessible
 * name instead. Same inline-SVG pattern as `InfoIcon.tsx`, sized in `em`
 * so it follows the link's own font size. */
export function ExternalLinkIcon() {
  return (
    <>
      {' '}
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="0.9em"
        height="0.9em"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        data-icon="external-link"
        style={{ verticalAlign: '-0.1em' }}
      >
        <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
        <polyline points="15 3 21 3 21 9" />
        <line x1="10" y1="14" x2="21" y2="3" />
      </svg>
      <VisuallyHidden>(opens in a new tab)</VisuallyHidden>
    </>
  );
}
