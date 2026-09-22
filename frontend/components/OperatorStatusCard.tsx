'use client';

import { Card, Group, Stack, Text } from '@mantine/core';
import { StatusBadge } from './StatusBadge';
import { LastUpdated } from './LastUpdated';
import { PinToggle } from './PinToggle';
import { TextLink } from './TextLink';
import { formatOperatorSampleSummary } from '@/lib/operatorStats';
import { isGoodSeverity } from '@/lib/severity';
import type { OperatorSummary } from '@/lib/types';

/** The "Worst of N lines · {name}" qualifier under the reason text (2026-09-22
 * UX review [OH] §2.3/I11): a rollup's reason is one line's own text
 * presented as the whole operator's status, with nothing saying it's the
 * worst of several or which one -- ScotRail's card was the clearest case,
 * one Fife Circle closure standing in for the whole operator. Linking the
 * named line to its own (unclamped) status page also gives a reader stuck
 * behind the reason's `lineClamp={3}` ellipsis somewhere to read the rest,
 * the same "click-through is the read-more" property `LineStatusCard`
 * already has (§2.2's other complaint about this card).
 *
 * Degrades gracefully when `worstLineId`/`worstLineName` are absent (an
 * older API payload, or -- per `lib/types.ts`'s own doc comment -- a
 * theoretically un-rolled-out backend): still states the scope ("Worst of 4
 * lines") without a name or a link, rather than rendering nothing. Says "N
 * lines, all running normally" instead of "Worst of N" when nothing is
 * actually wrong, per the review's own recommended copy -- "worst of" reads
 * as an accusation when every line is Good Service. */
function RollupScopeLine({ operator }: { operator: OperatorSummary }) {
  const lineCount = operator.lineIds.length;
  if (lineCount === 0) return null;
  const linesWord = lineCount === 1 ? 'line' : 'lines';

  if (isGoodSeverity(operator.worstSeverity)) {
    return (
      <Text size="xs" c="dimmed">
        {lineCount} {linesWord}, all running normally
      </Text>
    );
  }

  return (
    <Text size="xs" c="dimmed">
      Worst of {lineCount} {linesWord}
      {operator.worstLineId && operator.worstLineName ? (
        <>
          {' · '}
          <TextLink href={`/lines/${operator.worstLineId}`} underline="always" inline size="xs">
            {operator.worstLineName}
          </TextLink>
        </>
      ) : null}
    </Text>
  );
}

/** Mirrors `LineStatusCard`'s shape (worst-status badge, reason, sample
 * summary, last-updated) -- deliberately NOT wrapped in a `Link` the way
 * `LineStatusCard` links to `/lines/{id}`: there is no
 * `/operators/[code]` detail page in this phase for a click to go to (see
 * docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 7). Used by both `/operators`' list page and the
 * homepage's "Your Operators" section -- the same reuse `LineStatusCard`
 * already gets across the homepage's "Your Lines" section, the one other
 * place a status card like this renders. */
export function OperatorStatusCard({
  operator,
  pinned,
  needsAccountHint = false,
  showPin = true,
  dedupedLineId,
}: {
  operator: OperatorSummary;
  pinned: boolean;
  needsAccountHint?: boolean;
  /** Whether the pin star renders at all. Defaults to `true` -- every
   * existing call site (the `/operators` list, and the homepage's own
   * "Your Operators" section) keeps pinning available, since it is the
   * only way to pin/unpin an operator at all. Homepage review M9 ("only
   * one of the three card types carries a pin control") is resolved the
   * other direction there: `LineStatusCard` (the homepage's "Your Lines"
   * card) has no pin control of its own to add one to without a much
   * larger change, so parity is restored by giving THIS card the same
   * option to go without one, not by inventing pinning for the other. */
  showPin?: boolean;
  /** When set and equal to `operator.worstLineId`, the line actually
   * driving this rollup is already shown elsewhere on the same page (e.g.
   * the homepage's "Your Lines" section, a few hundred pixels above "Your
   * Operators") -- collapses the repeated reason text into a short
   * cross-reference instead of printing the same disruption a second time
   * (review M10). Left `undefined` (the default) everywhere else, since
   * `/operators`' own list page has no earlier section to be a duplicate
   * of. */
  dedupedLineId?: string;
}) {
  const reasonAlreadyShownAbove =
    dedupedLineId !== undefined && operator.worstLineId !== undefined && dedupedLineId === operator.worstLineId;

  return (
    <Card withBorder shadow="sm" padding="lg" style={{ display: 'flex', flexDirection: 'column' }}>
      <Stack gap="xs" style={{ flexGrow: 1 }}>
        <Group justify="space-between" wrap="nowrap" gap="xs" data-card-title-row>
          <Text fw={600} lineClamp={2} style={{ minWidth: 0 }}>
            {operator.name}
          </Text>
          <StatusBadge severity={operator.worstSeverity} />
        </Group>
        {reasonAlreadyShownAbove && operator.worstLineName ? (
          <Text size="sm" c="dimmed" data-card-reason>
            See{' '}
            <TextLink href={`/lines/${operator.worstLineId}`} underline="always" inline size="sm">
              {operator.worstLineName}
            </TextLink>{' '}
            above.
          </Text>
        ) : (
          <>
            <Text
              size="sm"
              c="dimmed"
              lineClamp={3}
              data-card-reason
              style={{ display: '-webkit-box', WebkitBoxOrient: 'vertical', WebkitLineClamp: 3, overflow: 'hidden' }}
            >
              {operator.reason || 'No current disruption reported.'}
            </Text>
            <RollupScopeLine operator={operator} />
          </>
        )}
        <Text size="xs" c="dimmed">
          {formatOperatorSampleSummary(operator)}
        </Text>
        {/* `marginTop: 'auto'` anchors this row to the card's bottom edge
            regardless of how many lines the name/reason above wrapped to --
            without it the footer (and the pin star inside it) drifted
            vertically between cards in the same row, sitting at different
            heights depending on how much text was above it (review M6). */}
        <Group justify="space-between" align="center" wrap="nowrap" data-card-footer style={{ marginTop: 'auto' }}>
          {operator.computedAt ? (
            <LastUpdated timestamp={operator.computedAt} />
          ) : (
            <Text size="xs" c="dimmed">
              &nbsp;
            </Text>
          )}
          {showPin && (
            <PinToggle
              kind="operator"
              id={operator.code}
              initiallyPinned={pinned}
              needsAccountHint={needsAccountHint}
            />
          )}
        </Group>
      </Stack>
    </Card>
  );
}
