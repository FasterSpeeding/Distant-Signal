import { Fragment } from 'react';
import { VisuallyHidden } from '@mantine/core';
import { ROUTE_ARROW } from '@/lib/stationLabel';

/** The "→" between two ends of a route ("KGX → YRK"). On screen it is the
 * arrow; to a screen reader it is the word "to" -- read literally, the
 * glyph comes out as "right arrow", which says nothing about a route. The
 * arrow is `aria-hidden` and a `VisuallyHidden` "to" stands in for it, so
 * the accessible text is "KGX to YRK" while the visible text is
 * unchanged. The caller keeps the spaces on either side, exactly where the
 * old literal arrow had them.
 *
 * Plain-text contexts that cannot hold markup -- a document title, an
 * `aria-label` -- use `spokenRoute` from `lib/stationLabel.ts` instead; a
 * `Select` renders its options through `renderOption` with `RouteText`. docs/style-guide.md ("Route arrows")
 * records the rule. */
export function RouteArrow() {
  return (
    <>
      <span aria-hidden="true" data-route-arrow>
        {ROUTE_ARROW}
      </span>
      <VisuallyHidden data-route-spoken>to</VisuallyHidden>
    </>
  );
}

/** Renders a route string built by `lib/stationLabel.ts` (`routeLabel`,
 * `codeRouteLabel`, and everything built on them) with each " → " swapped
 * for a `RouteArrow`. The string helpers stay plain strings -- they also
 * feed titles, option labels and rename placeholders -- and this is the
 * one place a rendered route turns its arrows into markup. */
export function RouteText({ children }: { children: string }) {
  const parts = children.split(ROUTE_SEPARATOR);
  return (
    <>
      {parts.map((part, i) => (
        <Fragment key={i}>
          {i > 0 && (
            <>
              {' '}
              <RouteArrow />{' '}
            </>
          )}
          {part}
        </Fragment>
      ))}
    </>
  );
}

const ROUTE_SEPARATOR = ` ${ROUTE_ARROW} `;
