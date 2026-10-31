import { Group, Stack, Text } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import { upcomingPeriod } from '@/components/TransitDisruptions';
import { formatTime } from '@/lib/dateFormat';
import { describeFailure } from '@/lib/failure';
import { transitIncidentHref } from '@/lib/transitStops';
import type { TransitLine, TransitLineStatus, TransitSituations, TransitStatusEntry } from '@/lib/types';

type Line = TransitLine & { status?: TransitLineStatus };

/** A line's published disruptions: the headline (its worst, as its
 * statuses come worst first) and how many more it has. */
export interface LineDisruptions {
  headline: TransitStatusEntry & { situationId: string };
  more: number;
}

function hasSituation(entry: TransitStatusEntry): entry is TransitStatusEntry & { situationId: string } {
  return entry.situationId !== null;
}

/** `line`'s disruptions, or null when it has none (or its status isn't
 * current). */
export function lineDisruptions(line: Line): LineDisruptions | null {
  if (line.status?.state !== 'current') return null;
  const entries = line.status.statuses.filter(hasSituation);
  const headline = entries[0];
  return headline ? { headline, more: entries.length - 1 } : null;
}

/** The lines with a published disruption, in the network's order. */
export function lineRows(lines: readonly Line[]): (LineDisruptions & { line: Line })[] {
  return lines.flatMap((line) => {
    const disruptions = lineDisruptions(line);
    return disruptions ? [{ ...disruptions, line }] : [];
  });
}

/** One line's disruption in a line: its status chip (unless `chip` is
 * false, for a row that shows the status already), its worst disruption's
 * headline linking that disruption's page, and "and N more" linking the
 * line's page. Nothing when the line has none. For a network page's line
 * list as much as for the disruptions summary below. */
export function TransitLineDisruption({
  network,
  line,
  chip = true,
  named = false,
}: {
  network: string;
  line: Line;
  chip?: boolean;
  /** Leads with the line's name ("Blue Line: ..."). */
  named?: boolean;
}) {
  const disruptions = lineDisruptions(line);
  if (!disruptions) return null;
  const { headline, more } = disruptions;
  return (
    <Group gap="xs" wrap="nowrap" align="flex-start" data-line-disruption>
      {chip && <StatusBadge severity={headline.severity} />}
      <Text size="sm" lineClamp={2}>
        {named && (
          <>
            <Text span fw={600} size="sm">
              {line.name}:
            </Text>{' '}
          </>
        )}
        <TextLink href={transitIncidentHref(network, headline.situationId)} inline underline="always" size="sm">
          {headline.reason}
        </TextLink>
        {more > 0 && (
          <>
            {' '}
            <TextLink
              href={`/transit/${encodeURIComponent(network)}/lines/${encodeURIComponent(line.lineId)}`}
              inline
              size="sm"
            >
              and {more} more
            </TextLink>
          </>
        )}
      </Text>
    </Group>
  );
}

function plural(count: number, one: string, other: string): string {
  return `${count} ${count === 1 ? one : other}`;
}

/** What the network has now and coming up, in one sentence. */
export function disruptionCount(current: number, upcoming: number): string {
  const now = current === 0 ? 'No disruptions reported now' : `${plural(current, 'disruption', 'disruptions')} now`;
  return upcoming === 0 ? `${now}.` : `${now}, ${upcoming} coming up.`;
}

/** A network page's disruptions, kept short: how many there are, then a
 * row per affected line (its status, its worst disruption's headline, and
 * a count of the rest), the notices no line row covers (a lift out of
 * service), and what is coming up. Every headline links its own page
 * (`/transit/<network>/incidents/<id>`), where the detail is. Nothing
 * while the network has no disruptions feed; "Status unavailable" once
 * they are over 30 minutes old. */
export function TransitDisruptionSummary({
  network,
  lines,
  situations,
  failed = false,
}: {
  network: string;
  lines: readonly Line[];
  situations: TransitSituations | null | undefined;
  /** Loading the disruptions failed (with a feed known to exist). */
  failed?: boolean;
}) {
  if (failed) {
    return (
      <Stack gap="sm">
        <SectionTitle>Disruptions</SectionTitle>
        <Text c="dimmed">{describeFailure('load', 'disruptions')}</Text>
      </Stack>
    );
  }
  if (!situations || situations.state === 'none') return null;
  if (situations.state === 'unavailable') {
    return (
      <Stack gap="sm">
        <SectionTitle>Disruptions</SectionTitle>
        <Text c="dimmed">
          Status unavailable.
          {situations.asOf ? ` Last read at ${formatTime(situations.asOf)}.` : ''}
        </Text>
      </Stack>
    );
  }
  const rows = lineRows(lines);
  const covered = new Set(rows.flatMap((row) => row.line.status?.statuses.map((s) => s.situationId) ?? []));
  const others = situations.current.filter((situation) => !covered.has(situation.id));
  const href = (id: string) => transitIncidentHref(network, id);
  return (
    <Stack gap="sm">
      <SectionTitle>Disruptions</SectionTitle>
      <Text c="dimmed">{disruptionCount(situations.current.length, situations.upcoming.length)}</Text>
      {rows.length > 0 && (
        <Stack component="ul" gap="xs" className="ds-plain-list" aria-label="Disruptions by line">
          {rows.map(({ line }) => (
            <li key={line.id}>
              <TransitLineDisruption network={network} line={line} named />
            </li>
          ))}
        </Stack>
      )}
      {others.length > 0 && (
        <Stack gap={4}>
          <Text fw={600} size="sm">
            Notices
          </Text>
          <Stack component="ul" gap={4} className="ds-plain-list" aria-label="Other notices">
            {others.map((situation) => (
              <Text component="li" key={situation.id} size="sm" lineClamp={2}>
                <TextLink href={href(situation.id)} inline underline="always" size="sm">
                  {situation.summary}
                </TextLink>
              </Text>
            ))}
          </Stack>
        </Stack>
      )}
      {situations.upcoming.length > 0 && (
        <Stack gap={4}>
          <Text fw={600} size="sm">
            Coming up
          </Text>
          <Stack component="ul" gap={4} className="ds-plain-list" aria-label="Disruptions coming up">
            {situations.upcoming.map((situation) => {
              const when = upcomingPeriod(situation);
              return (
                <Text component="li" key={situation.id} size="sm" lineClamp={2}>
                  {when && (
                    <Text span size="sm" c="dimmed">
                      {when}:{' '}
                    </Text>
                  )}
                  <TextLink href={href(situation.id)} inline underline="always" size="sm">
                    {situation.summary}
                  </TextLink>
                </Text>
              );
            })}
          </Stack>
        </Stack>
      )}
    </Stack>
  );
}
