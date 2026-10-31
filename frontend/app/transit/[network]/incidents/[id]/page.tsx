import { Fragment } from 'react';
import { Divider, Group, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { IncidentStateBadge } from '@/components/IncidentStateBadge';
import { SectionTitle } from '@/components/SectionTitle';
import { ShareButton } from '@/components/ShareButton';
import { StateBadge } from '@/components/StateBadge';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { getTransitNetwork, getTransitSituation, getTransitStops } from '@/lib/api';
import { formatDateTime, formatTime, TIMES_IN_UK_LOCAL_TIME } from '@/lib/dateFormat';
import { pageMetadata, SITE_NAME } from '@/lib/pageMetadata';
import { localLineId, orNotFound, TRANSIT_ID, transitIncidentHref, transitStopHref } from '@/lib/transit';
import {
  transitIncidentPreviewAlt,
  transitIncidentPreviewCard,
  transitIncidentPreviewDescription,
  transitIncidentPreviewImage,
} from '@/lib/transitIncidentPreview';
import type { TransitSituationDetail } from '@/lib/types';

interface Params {
  params: Promise<{ network: string; id: string }>;
}

// As `/incidents/[id]`: always rendered against the api, never prerendered.
export const revalidate = 0;

async function load(network: string, id: string) {
  if (!TRANSIT_ID.test(network) || id.length === 0 || id.length > 128) notFound();
  return Promise.all([orNotFound(getTransitNetwork(network)), orNotFound(getTransitSituation(network, id))]);
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network, id } = await params;
  const [detail, situation] = await load(network, id);
  const card = transitIncidentPreviewCard(situation, { networkName: detail.name, now: Date.now() });
  // One disruption, gone within days: worth following, not worth indexing.
  return pageMetadata(situation.summary, transitIncidentPreviewDescription(situation, detail.name), {
    path: transitIncidentHref(network, situation.id),
    noindex: true,
    image: transitIncidentPreviewImage(network, situation, transitIncidentPreviewAlt(card, SITE_NAME)),
  });
}

function formatPeriod(period: { start: string; end: string | null }): string {
  const from = formatDateTime(period.start);
  return period.end ? `${from} – ${formatDateTime(period.end)}` : `${from} – ongoing`;
}

/** Why an ended or upcoming disruption affects nothing now, in the
 * incident page's words. */
function notNow(situation: TransitSituationDetail): string | null {
  if (situation.state === 'upcoming') {
    return situation.starts ? `Not in effect yet: starts ${formatDateTime(situation.starts)}.` : 'Not in effect yet.';
  }
  if (situation.state !== 'ended') return null;
  if (situation.endedAt) return `No longer listed by the source since ${formatDateTime(situation.endedAt)}.`;
  if (situation.validityEndedAt) return `Its validity period ended ${formatDateTime(situation.validityEndedAt)}.`;
  return 'This disruption has ended.';
}

/** `/transit/[network]/incidents/[id]`: one published disruption of a
 * transit network (a Metrolink situation), as `/incidents/[id]` shows a
 * National Rail incident: its state, the lines it changes and how, the
 * stops it names, its advice and validity, and a share button and preview
 * card. It stays for a week after the feed stops listing it, marked
 * ended, so a shared link still explains itself. */
export default async function TransitIncidentPage({ params }: Params) {
  const { network, id } = await params;
  const [[detail, situation], stops] = await Promise.all([load(network, id), getTransitStops(network).catch(() => [])]);
  const stopNames = new Map(stops.map((stop) => [stop.id, stop.name]));
  const namedStops = situation.stops.flatMap((stopId) => {
    const name = stopNames.get(stopId);
    return name ? [{ id: stopId, name }] : [];
  });
  const stale = situation.state !== 'ended' && situation.status.state === 'unavailable';
  const why = notNow(situation);
  const infoUrl = situation.infoUrl && /^https?:\/\//i.test(situation.infoUrl) ? situation.infoUrl : null;

  return (
    <Stack p="lg" gap="md">
      <TextLink href={`/transit/${encodeURIComponent(network)}`} underline="always">
        Back to {detail.name}
      </TextLink>

      <Title order={1} size="h2" lineClamp={3} title={situation.summary}>
        {situation.summary}
      </Title>

      <Group justify="space-between">
        <Group gap="sm">
          {situation.state === 'active' && situation.affects.length > 0 && (
            <StatusBadge severity={situation.severity} />
          )}
          {situation.planned && <StateBadge tone="planned">Planned work</StateBadge>}
          <IncidentStateBadge
            isCleared={false}
            state={situation.state}
            sourceRemovedAt={situation.endedAt}
            startsAt={situation.starts}
            validityEndedAt={situation.validityEndedAt}
          />
        </Group>
        <ShareButton />
      </Group>

      {stale && (
        <Text c="dimmed">
          Status unavailable.
          {situation.status.asOf ? ` Last read at ${formatTime(situation.status.asOf)}.` : ''}
        </Text>
      )}

      {/* Plain text as published: React renders it as text, never HTML. */}
      {situation.description && situation.description !== situation.summary && <Text>{situation.description}</Text>}
      {situation.advice.map((advice) => (
        <Text key={advice}>{advice}</Text>
      ))}
      {infoUrl && (
        // The feed's link, so only a web address (never `javascript:`).
        <TextLink href={infoUrl} underline="always">
          More details from {detail.name}
        </TextLink>
      )}

      {namedStops.length > 0 && (
        <Stack gap={4}>
          <SectionTitle>Stops</SectionTitle>
          <Text c="dimmed">
            {namedStops.map((stop, i) => (
              <Fragment key={stop.id}>
                {i > 0 && (i === namedStops.length - 1 ? ' and ' : ', ')}
                <TextLink href={transitStopHref(network, stop.id)} inline underline="always">
                  {stop.name}
                </TextLink>
              </Fragment>
            ))}
          </Text>
        </Stack>
      )}

      <Stack gap={4}>
        <SectionTitle>Validity</SectionTitle>
        {situation.validity.length === 0 ? (
          <Text size="sm" c="dimmed">
            No end time given.
          </Text>
        ) : (
          situation.validity.map((period, i) => (
            <Text key={i} size="sm" c="dimmed">
              {formatPeriod(period)}
            </Text>
          ))
        )}
      </Stack>

      <Divider />

      {why ? (
        <Text size="sm" c="dimmed">
          {why}
        </Text>
      ) : (
        <Stack gap={4}>
          <SectionTitle>Currently affects</SectionTitle>
          {situation.affects.length === 0 ? (
            <Text size="sm" c="dimmed">
              Doesn&apos;t change any line&apos;s status.
            </Text>
          ) : (
            <Stack component="ul" gap="xs" className="ds-plain-list" aria-label="Lines affected">
              {situation.affects.map((line) => (
                <Group component="li" key={line.id} gap="sm" wrap="nowrap">
                  <StatusBadge severity={line.severity} />
                  <TextLink href={`/transit/${encodeURIComponent(network)}/lines/${localLineId(line.id)}`}>
                    {line.name}
                  </TextLink>
                </Group>
              ))}
            </Stack>
          )}
        </Stack>
      )}

      <Divider />

      <Stack gap={2}>
        <Text size="xs" c="dimmed">
          First seen: {formatDateTime(situation.firstSeenAt)}
        </Text>
        <Text size="xs" c="dimmed">
          Last updated: {formatDateTime(situation.updatedAt)}
        </Text>
        <Text size="xs" c="dimmed">
          {TIMES_IN_UK_LOCAL_TIME}
        </Text>
      </Stack>

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
