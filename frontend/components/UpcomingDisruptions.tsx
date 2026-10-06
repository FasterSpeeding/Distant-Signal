import { Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';
import { formatUpcomingWhen } from '@/lib/upcoming';
import type { UpcomingDisruption } from '@/lib/types';

/** A line's upcoming disruptions (`report.upcoming`): announced, not yet
 * started, and never part of the line's status or severity -- so they are
 * shown as a dimmed note beside it rather than as an issue.
 *
 * `compact` (the line card, which is itself a link) shows the soonest one
 * as plain text plus a count of the rest; the line page lists them all,
 * each linking to its incident. Renders nothing when there are none. */
export function UpcomingDisruptions({
  upcoming,
  compact = false,
}: {
  upcoming: UpcomingDisruption[] | undefined;
  compact?: boolean;
}) {
  const next = upcoming?.[0];
  if (!upcoming || next === undefined) {
    return null;
  }
  if (compact) {
    const rest = upcoming.length - 1;
    return (
      <Text size="xs" c="dimmed" lineClamp={2} data-upcoming>
        <Text span size="xs" fw={600}>
          Upcoming:
        </Text>{' '}
        {formatUpcomingWhen(next)}, {next.summary}
        {rest > 0 && ` (+${rest} more)`}
      </Text>
    );
  }
  return (
    <Stack gap={4} data-upcoming>
      <Text fw={500}>Upcoming</Text>
      <Text size="sm" c="dimmed">
        Announced, not yet in effect. Times in UK local time.
      </Text>
      {upcoming.map((note) => (
        <Text size="sm" key={`${note.incidentId}-${note.from}`}>
          <Text span size="sm" fw={600}>
            {formatUpcomingWhen(note)}
          </Text>
          {': '}
          <TextLink href={`/incidents/${encodeURIComponent(note.incidentId)}`} underline="always" inline>
            {note.summary}
          </TextLink>
        </Text>
      ))}
    </Stack>
  );
}
