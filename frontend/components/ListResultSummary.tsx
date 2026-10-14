'use client';

import { Text, UnstyledButton, VisuallyHidden } from '@mantine/core';

/** A filtered list's count and its way back: "3 lines match. Show all
 * lines" (docs/style-guide.md, "List search and filters"), including the
 * zero-row case. `onReset` is a real click handler, not a link: the
 * filters are client state, so a plain `<Link>` would change the address
 * bar without the mounted list re-reading it.
 *
 * The sentence is announced from an always-mounted, visually hidden
 * `role="status"` region, so a screen reader hears it change as the
 * visitor types or filters (a live region inserted together with its text
 * is often not announced). Visually hidden is absolutely positioned, so it
 * adds no Stack gap when empty. The visible copy is `aria-hidden` so it
 * isn't read twice. `announcement` overrides what the region says (a
 * server-backed list says "Searching" while it fetches) without changing
 * the visible line. */
export function ListResultSummary({
  visible,
  summary,
  resetLabel,
  onReset,
  announcement,
}: {
  visible: boolean;
  summary: string;
  resetLabel: string;
  onReset: () => void;
  announcement?: string | undefined;
}) {
  return (
    <>
      <VisuallyHidden role="status">{announcement ?? (visible ? summary : '')}</VisuallyHidden>
      {visible && (
        <Text size="sm" c="dimmed">
          <span aria-hidden="true">{summary} </span>
          <UnstyledButton onClick={onReset} className="textButton">
            {resetLabel}
          </UnstyledButton>
        </Text>
      )}
    </>
  );
}
