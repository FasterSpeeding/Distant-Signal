import type { ReactNode } from 'react';
import { Card, Group, Stack, Text } from '@mantine/core';
import { CardTitleLink, ClickableCard } from './ClickableCard';
import { tokens } from '@/lib/theme';

/** The one status card (round 2, item 3): a name, its status badge, the
 * reason, then the card's own details and an optional footer. Used for
 * lines (`LineStatusCard`), operators (`OperatorStatusCard`) and the
 * /status mode breakdown, on Status, Lines, Operators and the home page.
 *
 * With `href`, the whole card is clickable (`ClickableCard`): the title is
 * the link, stretched over the card, so the link is named by the title
 * alone and a link inside the card (an upcoming incident) stays its own.
 * Without it, the card is flat (`tokens.card.static`) because it holds
 * its own controls, such as a pin.
 *
 * The title row never wraps: a long name shrinks and clamps to two lines
 * so the badge is in the same place on every card in a grid. The reason
 * clamps to three lines; the detail page carries the rest. The footer sits
 * on the card's bottom edge however much text is above it, so footers line
 * up across a row. */
export function StatusCard({
  title,
  badge,
  reason,
  children,
  footer,
  href,
  ariaLabel,
}: {
  title: ReactNode;
  /** The severity, as a `StatusBadge`. None when there is nothing to
   * rate (a slice with no lines). */
  badge?: ReactNode;
  reason?: ReactNode;
  /** Small dimmed detail lines under the reason. */
  children?: ReactNode;
  footer?: ReactNode;
  href?: string;
  ariaLabel?: string;
}) {
  const body = (
    <Stack gap="xs" flex="1 1 auto">
      <Group justify="space-between" wrap="nowrap" gap="xs" align="flex-start" data-card-title-row data-wrap="nowrap">
        <Text fw={600} lineClamp={tokens.statusCard.titleLineClamp} miw={0}>
          {href !== undefined ? (
            <CardTitleLink href={href} ariaLabel={ariaLabel}>
              {title}
            </CardTitleLink>
          ) : (
            title
          )}
        </Text>
        {badge}
      </Group>
      {reason !== undefined && (
        <Text size="sm" c="dimmed" lineClamp={tokens.statusCard.reasonLineClamp} data-card-reason>
          {reason}
        </Text>
      )}
      {children}
      {footer !== undefined && (
        <Group justify="space-between" align="center" wrap="nowrap" data-card-footer mt="auto">
          {footer}
        </Group>
      )}
    </Stack>
  );

  if (href !== undefined) {
    return (
      <ClickableCard padding={tokens.statusCard.padding} fill>
        {body}
      </ClickableCard>
    );
  }
  return (
    <Card {...tokens.card.static} padding={tokens.statusCard.padding} className="ds-column" h="100%">
      {body}
    </Card>
  );
}
