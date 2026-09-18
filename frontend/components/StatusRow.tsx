import type { ReactNode } from 'react';
import { Box, Group, Stack, Text, type MantineSize } from '@mantine/core';

export type StatusRowProps = {
  /** The row's heading. A plain string (or number) is wrapped in a `Text`
   * that carries this component's own `fw={500}`/`lineClamp`/
   * `minWidth: 0` — `fw={500}` matches every one of this row's current
   * plain-string title call sites (`TrackedTrainSummaryRow`,
   * `SharedTrainSummaryRow`, `SharedCustomLineSummaryRow`, `SharedTrainRow`,
   * `TrackedTrainListRow`'s inner row), NOT `LineStatusCard.tsx`'s own
   * `fw={600}` — that card isn't built on `StatusRow` and this task
   * doesn't touch it, so there's no live caller that wants 600 here. Pass
   * a pre-built node instead (e.g. a `Link`-wrapped `Text`, as
   * `SharedCustomLineRow` needs) when the title itself has to be
   * interactive or otherwise isn't a bare string; in that case this
   * component only supplies the shrinkable, `minWidth: 0` container
   * around it, and the caller is responsible for its own weight/clamping
   * (both current composite-title call sites use `fw={500}` too, so the
   * row title's weight reads the same regardless of which path a given
   * row takes). */
  title: ReactNode;
  /** Optional second line under the title (e.g. "Shared by ..."). When
   * given, title+subtitle are stacked with `gap={4}`, matching every one
   * of this row's current call sites. */
  subtitle?: ReactNode;
  /** The status badge / action button(s) at the row's end. Nullish or
   * `false` (e.g. `canRemove && <Button .../>`) renders nothing — no
   * empty shrink-guard wrapper is emitted. This is the element the whole
   * component exists to protect: it gets `flexShrink: 0` so a long title
   * can never squeeze it to nothing or off-screen (WCAG 2.5.3). */
  trailing?: ReactNode;
  /** How many lines a string `title` may wrap to before it's clipped with
   * an ellipsis. Only applies when `title` is a string/number; ignored
   * for a pre-built title node. Matches `LineStatusCard`'s own default. */
  titleLineClamp?: number;
  /** Passed straight through to the outer `Group`'s `align`. Defaults to
   * `"center"`. A row whose title stacks a subtitle (or other multi-line
   * content) under it, and wants the trailing content pinned to the
   * title's line rather than the block's vertical middle, should pass
   * `"flex-start"` explicitly — same choice `SharedCustomLineRow`/
   * `track/mine`'s rows already made by hand before this component
   * existed. */
  align?: 'center' | 'flex-start' | 'flex-end' | 'stretch';
  /** Gap between the title/subtitle block and the trailing content.
   * Defaults to `"xs"`, matching `LineStatusCard`. */
  gap?: MantineSize;
  /** Extra props spread onto the outer `Group`, mainly for tests/CSS
   * hooks (e.g. `data-testid`). `wrap="nowrap"` and `justify` are fixed —
   * they're this component's whole reason to exist — so they aren't
   * overridable here. */
  'data-testid'?: string;
};

/** A "title (+ optional subtitle) plus trailing status/action content" row
 * — the shrink-guard convention `LineStatusCard.tsx` and `IssueList.tsx`
 * already established by hand, factored out once so it can't be
 * re-broken at a new call site.
 *
 * The bug this fixes (WCAG 2.5.3 for the button case): a `Group
 * wrap="nowrap"` row pairing a title with a badge/button, where neither
 * side has an explicit sizing rule, lets a long title squeeze the
 * trailing element down to nothing or off-screen instead of the title
 * itself truncating. Flexbox's default `min-width: auto` on a flex item
 * is the actual mechanism: without `minWidth: 0` on the title, the
 * *title* refuses to shrink below its content size first, so the
 * trailing element pays for that with the row's own leftover space
 * (which can go negative). This component fixes both ends at once: the
 * title gets `minWidth: 0` (so it's the one that gives), and the
 * trailing content gets `flexShrink: 0` (so it never gives).
 *
 * `data-status-row`/`data-wrap="nowrap"` mirror `LineStatusCard`'s own
 * convention of exposing `wrap` as a plain DOM attribute (Mantine only
 * ever turns it into the `--group-wrap` CSS var) so a render test can
 * assert on it directly without reading computed styles.
 */
export function StatusRow({
  title,
  subtitle,
  trailing,
  titleLineClamp = 2,
  align = 'center',
  gap = 'xs',
  'data-testid': dataTestId,
}: StatusRowProps) {
  const titleNode =
    typeof title === 'string' || typeof title === 'number' ? (
      <Text fw={500} lineClamp={titleLineClamp} style={{ minWidth: 0 }} data-status-row-title>
        {title}
      </Text>
    ) : (
      title
    );

  return (
    <Group
      justify="space-between"
      wrap="nowrap"
      align={align}
      gap={gap}
      data-status-row
      data-wrap="nowrap"
      data-testid={dataTestId}
    >
      <Box style={{ minWidth: 0, flexGrow: 1 }}>
        {subtitle ? (
          <Stack gap={4}>
            {titleNode}
            {subtitle}
          </Stack>
        ) : (
          titleNode
        )}
      </Box>
      {trailing != null && trailing !== false && (
        <Box style={{ flexShrink: 0 }} data-status-row-trailing>
          {trailing}
        </Box>
      )}
    </Group>
  );
}
