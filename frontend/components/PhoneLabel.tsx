import type { ReactNode } from 'react';

/** A label with a shorter phone form, for a badge or a column header in a
 * narrow table: `full` from `sm` up, `compact` below it
 * (`.phoneLabel__*` in `app/globals.css`). Only `full` is ever in the
 * accessibility tree: below `sm` it is visually hidden rather than
 * removed, and `compact` is `aria-hidden`, so a screen reader hears "4 min
 * late" or "Platform" whichever form is on screen. */
export function PhoneLabel({ full, compact }: { full: ReactNode; compact: ReactNode }) {
  return (
    <>
      <span className="phoneLabel__full">{full}</span>
      <span className="phoneLabel__compact" aria-hidden="true">
        {compact}
      </span>
    </>
  );
}
