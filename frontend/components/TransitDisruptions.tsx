import { Stack, Text } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { formatDateTime, formatTime } from '@/lib/dateFormat';
import { describeFailure } from '@/lib/failure';
import type { TransitSituation, TransitSituations } from '@/lib/types';

/** When a current situation runs: until its end, or since its start. */
function currentPeriod(situation: TransitSituation): string | null {
  const now = Date.now();
  const period =
    situation.validity.find((p) => Date.parse(p.start) <= now && (!p.end || Date.parse(p.end) > now)) ??
    situation.validity[0];
  if (!period) return null;
  return period.end ? `Until ${formatDateTime(period.end)}` : `Since ${formatDateTime(period.start)}`;
}

/** When an upcoming situation starts, and ends if it says. */
function upcomingPeriod(situation: TransitSituation): string | null {
  if (!situation.starts) return null;
  const period = situation.validity.find((p) => p.start === situation.starts);
  const from = `From ${formatDateTime(situation.starts)}`;
  return period?.end ? `${from} to ${formatDateTime(period.end)}` : from;
}

function Situation({
  situation,
  when,
  lineNames,
}: {
  situation: TransitSituation;
  when: string | null;
  lineNames?: Record<string, string> | undefined;
}) {
  const lines = lineNames
    ? situation.lines.map((id) => lineNames[id]).filter((name): name is string => Boolean(name))
    : [];
  return (
    <Stack component="li" gap={2}>
      <Text fw={600}>{situation.summary}</Text>
      {lines.length > 0 && (
        <Text size="sm" c="dimmed">
          {lines.join(', ')}
        </Text>
      )}
      {when && (
        <Text size="sm" c="dimmed">
          {when}
        </Text>
      )}
      {/* Plain text as published: React renders it as text, never HTML. */}
      {situation.description && situation.description !== situation.summary && (
        <Text size="sm">{situation.description}</Text>
      )}
      {situation.advice.map((advice) => (
        <Text key={advice} size="sm">
          {advice}
        </Text>
      ))}
      {situation.infoUrl && /^https?:\/\//i.test(situation.infoUrl) && (
        // The feed's link, so only a web address (never `javascript:`).
        <TextLink href={situation.infoUrl}>More details</TextLink>
      )}
    </Stack>
  );
}

/** A network's or a line's published disruptions: those happening now,
 * then those coming up within a week. Nothing while the network has no
 * disruptions feed; "Status unavailable" once they are over 30 minutes
 * old, since stale disruption news reads as current. */
export function TransitDisruptions({
  situations,
  empty,
  lineNames,
  failed = false,
  title = 'Disruptions',
}: {
  situations: TransitSituations | null | undefined;
  /** What an empty "now" list says. */
  empty: string;
  /** Public line id to name, to say which lines each one affects. */
  lineNames?: Record<string, string> | undefined;
  /** Loading them failed (with a feed known to exist). */
  failed?: boolean;
  title?: string;
}) {
  if (failed) {
    return (
      <Stack gap="sm">
        <SectionTitle>{title}</SectionTitle>
        <Text c="dimmed">{describeFailure('load', 'disruptions')}</Text>
      </Stack>
    );
  }
  if (!situations || situations.state === 'none') return null;
  return (
    <Stack gap="sm">
      <SectionTitle>{title}</SectionTitle>
      {situations.state === 'unavailable' ? (
        <Text c="dimmed">
          Status unavailable.
          {situations.asOf ? ` Last read at ${formatTime(situations.asOf)}.` : ''}
        </Text>
      ) : (
        <>
          {situations.current.length === 0 ? (
            <Text c="dimmed">{empty}</Text>
          ) : (
            <Stack component="ul" gap="md" className="ds-plain-list" aria-label="Disruptions now">
              {situations.current.map((s) => (
                <Situation key={s.id} situation={s} when={currentPeriod(s)} lineNames={lineNames} />
              ))}
            </Stack>
          )}
          {situations.upcoming.length > 0 && (
            <Stack gap="sm">
              <Text fw={600} size="sm">
                Coming up
              </Text>
              <Stack component="ul" gap="md" className="ds-plain-list" aria-label="Disruptions coming up">
                {situations.upcoming.map((s) => (
                  <Situation key={s.id} situation={s} when={upcomingPeriod(s)} lineNames={lineNames} />
                ))}
              </Stack>
            </Stack>
          )}
        </>
      )}
    </Stack>
  );
}

/** A stop's notices (a lift out of service, the stop closed): its current
 * disruptions. Nothing when there are none. */
export function TransitStopNotices({ notices }: { notices: TransitSituation[] | undefined }) {
  if (!notices || notices.length === 0) return null;
  return (
    <Stack gap="sm">
      <SectionTitle>Notices</SectionTitle>
      <Stack component="ul" gap="md" className="ds-plain-list" aria-label="Notices">
        {notices.map((s) => (
          <Situation key={s.id} situation={s} when={currentPeriod(s)} />
        ))}
      </Stack>
    </Stack>
  );
}
