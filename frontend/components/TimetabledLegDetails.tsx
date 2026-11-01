import { Group, Stack, Text } from '@mantine/core';
import { StateBadge } from './StateBadge';
import { StatusBadge } from './StatusBadge';
import { TextLink } from './TextLink';
import { TransitLineSwatch } from './TransitLineSwatch';
import { formatTime } from '@/lib/dateFormat';
import { noticeHref, timetableCaption, timetableNote, timetabledLineHref } from '@/lib/timetabledLeg';
import type { JourneyLegDetail, LegTimetable } from '@/lib/types';

/** What a timetabled leg's network timetable says about it (DS-237): its
 * line, where it is heading, any disruption on the line, a note when the
 * timetable no longer has the ride as planned, and its timetabled stops.
 * Scheduled times only: no progress, no expected times, nothing that
 * reads as live. */
export function TimetabledLegDetails({ leg, timetable }: { leg: JourneyLegDetail; timetable: LegTimetable }) {
  const done = leg.untrackedStatus === 'done';
  const note = timetableNote(timetable, leg.transitMode, done);
  const lineHref = timetabledLineHref(timetable);
  const headsign = timetable.headsign ?? leg.headsign;
  return (
    <Stack gap="xs" data-timetabled-leg>
      {(timetable.line !== null || !!headsign) && (
        <Group gap="xs" wrap="wrap">
          {timetable.line && (
            <Text size="sm">
              <TransitLineSwatch line={timetable.line} size={10} inline />
              {lineHref ? (
                <TextLink href={lineHref} underline="always" inline size="sm">
                  {timetable.line.name}
                </TextLink>
              ) : (
                timetable.line.name
              )}
            </Text>
          )}
          {headsign && <Text size="sm">Towards {headsign}</Text>}
        </Group>
      )}
      {!done &&
        timetable.notices.map((notice, index) => {
          const href = noticeHref(timetable.network, notice);
          return (
            <Stack key={index} gap={2} data-timetabled-leg-notice>
              <Group gap="xs" wrap="wrap">
                <StatusBadge severity={notice.severity} />
                <Text size="sm">{notice.message}</Text>
              </Group>
              {notice.reason && (
                <Text size="xs" c="dimmed">
                  {notice.reason}
                </Text>
              )}
              {href && (
                <TextLink href={href} underline="always" size="xs">
                  More about this disruption
                </TextLink>
              )}
            </Stack>
          );
        })}
      {note && (
        <Group gap="xs" wrap="wrap" align="flex-start" data-timetable-note={timetable.state}>
          {note.tone === 'caution' && <StateBadge tone="caution">Check times</StateBadge>}
          <Text size="sm">{note.text}</Text>
        </Group>
      )}
      {timetable.calls.length >= 2 && (
        <Stack gap={2} data-timetabled-calls>
          <Text size="xs" c="dimmed">
            Timetabled stops
          </Text>
          {timetable.calls.map((call) => {
            const time = call.scheduledDeparture ?? call.scheduledArrival;
            return (
              <Group key={`${call.stopId}-${time}`} gap="sm" wrap="nowrap" data-timetabled-call>
                <Text size="sm" miw="3rem">
                  {time ? formatTime(time) : ''}
                </Text>
                <Text size="sm">
                  {call.name}
                  {call.platform && (
                    <Text span size="xs" c="dimmed">
                      {' '}
                      ({call.platform})
                    </Text>
                  )}
                </Text>
              </Group>
            );
          })}
        </Stack>
      )}
      <Text size="xs" c="dimmed">
        {timetableCaption(timetable, leg.transitMode)}
      </Text>
    </Stack>
  );
}
