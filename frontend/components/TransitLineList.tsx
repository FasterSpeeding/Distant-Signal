import type { ReactNode } from 'react';
import { Box, Stack, Text } from '@mantine/core';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';
import { StatusRow } from '@/components/StatusRow';
import { TransitLineSwatch } from '@/components/TransitLineSwatch';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { isGoodSeverity } from '@/lib/severity';
import type { TransitLine, TransitLineStatus } from '@/lib/types';
import classes from './TransitLineList.module.css';

type ListedLine = TransitLine & { status?: TransitLineStatus };

/** A network's lines as compact rows, the page's main way in: one card
 * per line, its colour, name (the link to its page, stretched over the
 * card) and status badge on one line, then its disruption summary. One
 * column on a phone, two from `sm`, three from `lg`, so seven Metrolink
 * lines or eleven Tube lines read at a glance.
 *
 * `summary` fills each row's second line: the network page passes the
 * line's disruption summary there. Without one, a line whose status isn't
 * good says why, in a line (`TransitLineStatusSummary`). */
export function TransitLineList({
  network,
  lines,
  label,
  summary = (line) => <TransitLineStatusSummary status={line.status} lineName={line.name} />,
}: {
  network: string;
  lines: ListedLine[];
  /** The list's accessible name ("Underground lines"). */
  label: string;
  summary?: (line: ListedLine) => ReactNode;
}) {
  return (
    <Box component="ul" className={`ds-plain-list ${classes.grid}`} aria-label={label}>
      {lines.map((line) => (
        <li key={line.id} className={classes.item}>
          <ClickableCard padding="xs" fill>
            <Stack gap={2}>
              <StatusRow
                title={
                  <Box className={classes.name}>
                    <TransitLineSwatch line={line} />
                    <Text fw={600} lineClamp={1} miw={0}>
                      <CardTitleLink href={`/transit/${network}/lines/${line.lineId}`}>{line.name}</CardTitleLink>
                    </Text>
                  </Box>
                }
                trailing={<TransitStatusBadge status={line.status} />}
              />
              {summary(line)}
            </Stack>
          </ClickableCard>
        </li>
      ))}
    </Box>
  );
}

/** A line's status in a line, under its badge: the first status's reason
 * that isn't good service ("Minor delays between Bank and Morden"), and how
 * many more it has. Nothing while the line runs well, or has no feed.
 * The placeholder for the per-line disruption summary the incident pages
 * bring (DS-228). TfL starts its reasons with the line's name
 * ("PICCADILLY LINE: ..."), which the card already shows, so a leading
 * "<line name>:" is dropped. */
export function TransitLineStatusSummary({
  status,
  lineName,
}: {
  status?: TransitLineStatus | null | undefined;
  lineName?: string | undefined;
}) {
  if (status?.state !== 'current') return null;
  const problems = status.statuses.filter((entry) => !isGoodSeverity(entry.severity));
  const first = problems[0];
  const raw = first?.reason.trim() || first?.description.trim();
  if (!raw) return null;
  const text = lineName ? withoutLinePrefix(raw, lineName) : raw;
  const more = problems.length - 1;
  return (
    <Text size="sm" c="dimmed" lineClamp={2} data-line-summary>
      {text}
      {more > 0 && ` (and ${more} more)`}
    </Text>
  );
}

/** `text` without a leading "<line name>[ line]:" ("Bakerloo Line: ..."),
 * matched ignoring case; unchanged when it has none or nothing follows. */
export function withoutLinePrefix(text: string, lineName: string): string {
  const colon = text.indexOf(':');
  if (colon <= 0) return text;
  const prefix = text.slice(0, colon).trim().toLowerCase();
  const name = lineName.trim().toLowerCase();
  if (prefix !== name && prefix !== `${name} line`) return text;
  const rest = text.slice(colon + 1).trim();
  return rest || text;
}
