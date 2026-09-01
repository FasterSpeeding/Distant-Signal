import { Badge, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTrackedTrains } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { TrackedTrainListItem } from '@/lib/types';

// See app/page.tsx's own `revalidate = 0` comment for the rationale: this
// route has no dynamic segment, so without this Next.js treats it as
// eligible for static generation and tries to prerender it during `next
// build`, which fails since the `api` service only exists on the compose
// network at runtime.
export const revalidate = 0;

/** `/track/mine` -- a logged-in user's own tracked trains, most-recently-
 * tracked first, per
 * docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md
 * Decision 3. `getMyTrackedTrains()` returning `null` on a `401` is the
 * COMPLETE "not logged in" signal for this page -- unlike
 * `components/TicketPanel.tsx`, no separate `getSession()` call is needed
 * here, since there's no second party (owner vs. not) to disambiguate on
 * a route with no id in its path (Decision 3's own note). */
export default async function MyTrackedTrainsPage() {
  const trains = await getMyTrackedTrains();

  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Tracked Trains</Title>
        <LoginLink underline="always">
          Log in to see the trains you&apos;re tracking
        </LoginLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between" align="baseline">
        <Title order={1}>My Tracked Trains</Title>
        <TextLink href="/track">Track a new train</TextLink>
      </Group>
      {trains.length === 0 ? (
        <Text c="dimmed">
          You haven&apos;t tracked any trains yet. <Link href="/track">Track a train</Link> to get started.
        </Text>
      ) : (
        <Stack gap="xs">
          {trains.map((train) => (
            <TrackedTrainListRow key={train.id} train={train} />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function TrackedTrainListRow({ train }: { train: TrackedTrainListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- matching the existing by-id page's own "canonical link
  // once resolved" logic rather than always sending the user through the
  // by-id redirect hop. The `resolved`-with-null-`trainUid` fallback is
  // defensive: the backend's own resolution invariant means this
  // shouldn't happen, but this component doesn't assume it.
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = train.pinDestinationCrs
    ? `${train.pinOriginCrs} → ${train.pinDestinationCrs}`
    : train.pinOriginCrs;

  return (
    <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder>
        <Stack gap={4}>
          <Group justify="space-between" wrap="nowrap">
            <Text fw={500}>{route}</Text>
            <RowStatusBadge train={train} />
          </Group>
          <Text size="sm" c="dimmed">
            {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

// Short, human badge words for the raw enum tokens this page can receive --
// `resolutionStatus` (`pending`/`unresolved`) and journey `status`
// (`awaiting_activation`/`en_route`/`completed`/`cancelled`). Kept local to
// this file rather than reused from `TrainJourney.tsx`: that component's
// equivalent branching renders full sentences for a detail page's
// `Alert`/prose, not a short word for a list-row `Badge`. Falls back to the
// raw token itself for anything unlisted, so an unexpected value never
// disappears from the badge.
const STATUS_LABELS: Record<string, string> = {
  pending: 'Pending match',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

function RowStatusBadge({ train }: { train: TrackedTrainListItem }) {
  // `pending`/`unresolved` show the resolution status itself -- no
  // journey status exists yet for either. Once `resolved`, the journey
  // `status` plus a delay badge takes over, reusing the same "Xm
  // late"/"On time" treatment `TrainJourney.tsx`'s `JourneyDetails`
  // already uses. No "active only" filter and no attempt to distinguish
  // a genuinely-finished journey from one that's merely gone quiet -- per
  // Decision 2/Finding 1 of the design spec, the backend can't honestly
  // support that distinction today.
  if (train.resolutionStatus !== 'resolved') {
    return (
      <Badge color={train.resolutionStatus === 'unresolved' ? 'red' : 'gray'} variant="light">
        {STATUS_LABELS[train.resolutionStatus] ?? train.resolutionStatus}
      </Badge>
    );
  }
  return (
    <Group gap={6} wrap="nowrap">
      {train.status && (
        // Cancelled is the one state this at-a-glance triage page must
        // make visually distinct -- everything else (en route, completed,
        // awaiting activation) stays the neutral gray a running/finished
        // train shares, matching the single-train detail page's red
        // `Alert` treatment of the same status (`TrainJourney.tsx`).
        <Badge color={train.status === 'cancelled' ? 'red' : 'gray'} variant="light">
          {STATUS_LABELS[train.status] ?? train.status}
        </Badge>
      )}
      {train.delayMinutes !== null && (
        <Badge color={train.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
          {train.delayMinutes > 0 ? `${train.delayMinutes}m late` : 'On time'}
        </Badge>
      )}
    </Group>
  );
}
