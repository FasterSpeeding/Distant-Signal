import type { ReactNode } from 'react';
import { Box, Stack, Text } from '@mantine/core';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';
import { StatusRow } from '@/components/StatusRow';
import { TransitLineSwatch } from '@/components/TransitLineSwatch';
import classes from './LineStatusRow.module.css';

/** The compact line-status row (docs/style-guide.md, "Line status rows"):
 * one card per line, its colour (when it has one), its name (the card's
 * link, stretched over it, clamped to a line) and its status badge on one
 * row, then at most a line or two of detail. For a list of lines to scan:
 * a transit network's lines, a station's lines. `LineStatusCard` is the
 * fuller card for a status dashboard (its reason, sample summary, upcoming
 * disruptions, when it was updated). */
export function LineStatusRow({
  href,
  name,
  colour,
  badge,
  children,
}: {
  href: string;
  name: string;
  /** The line's colour swatch; omitted for a line with none. */
  colour?: { colour: string | null } | undefined;
  /** A `StatusBadge` or `TransitStatusBadge`; nothing to show none. */
  badge?: ReactNode;
  /** The row's detail, small and dimmed, under the name. */
  children?: ReactNode;
}) {
  return (
    <ClickableCard padding="xs" fill>
      <Stack gap={2}>
        <StatusRow
          title={
            <Box className={classes.name}>
              {colour && <TransitLineSwatch line={colour} />}
              <Text fw={600} lineClamp={1} miw={0}>
                <CardTitleLink href={href}>{name}</CardTitleLink>
              </Text>
            </Box>
          }
          trailing={badge}
        />
        {children}
      </Stack>
    </ClickableCard>
  );
}

/** `LineStatusRow`s as a list: one column on a phone, two from `sm`, three
 * from `lg` (`columns={2}` stops at two), every row in a line as tall as
 * the tallest. */
export function LineStatusRowList({
  label,
  columns = 3,
  children,
}: {
  /** The list's accessible name ("Underground"). */
  label: string;
  columns?: 2 | 3;
  children: ReactNode;
}) {
  return (
    <Box component="ul" className={`ds-plain-list ${classes.grid}`} data-columns={columns} aria-label={label}>
      {children}
    </Box>
  );
}

/** One item of a `LineStatusRowList`. */
export function LineStatusRowItem({ children }: { children: ReactNode }) {
  return <li className={classes.item}>{children}</li>;
}

/** A row's detail line: small, dimmed, clamped to two lines. */
export function LineStatusRowDetail({ children, ...rest }: { children: ReactNode; 'data-line-summary'?: boolean }) {
  return (
    <Text size="sm" c="dimmed" lineClamp={2} {...rest}>
      {children}
    </Text>
  );
}
