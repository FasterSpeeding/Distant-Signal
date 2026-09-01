import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTrackedTrains, getMyTickets } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { TicketSummary } from '@/components/TicketSummary';
import { DelayRepayEstimate } from '@/components/DelayRepayEstimate';
import { TicketEntryForm } from '@/components/TicketEntryForm';
import { AttachTicketAction } from '@/components/AttachTicketAction';
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { TrackedTrainListItem, TicketListItem } from '@/lib/types';

// See app/page.tsx's own `revalidate = 0` comment for the rationale: this
// route has no dynamic segment, so without this Next.js treats it as
// eligible for static generation and tries to prerender it during `next
// build`, which fails since the `api` service only exists on the compose
// network at runtime.
export const revalidate = 0;

/** `/track/mine` -- a logged-in user's own tracked trains AND tickets, one
 * merged page (Part B of the upload-first ticket-tracking plan). Was two
 * separate pages (`/track/mine` for trains, `/track/tickets` for tickets)
 * per docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md and
 * docs/superpowers/specs/2026-08-31-tickets-list-design.md -- the latter's
 * own Decision 3 gave "a ticket-focused view answers a different question
 * than a train-focused one" as its reason to keep them apart, but that
 * reasoning assumed every ticket has an owning tracked train. Part A of
 * this plan removed that assumption (a ticket can now exist standalone,
 * `trackedTrainId: null`, before a tracked train exists for it), and once
 * that's true, a bare "My Tickets" list of mostly-attached tickets sits
 * awkwardly next to a bare "My Tracked Trains" list -- the natural, useful
 * view is "my trains, each with whatever ticket(s) I've attached to it"
 * plus "tickets I haven't attached to anything yet, with a way to do that
 * now." `/track/tickets` now redirects here rather than duplicating this
 * page's content under two URLs.
 *
 * `getMyTrackedTrains()` returning `null` on a `401` is the COMPLETE "not
 * logged in" signal for this page, same as before -- there's no second
 * party to disambiguate on a route with no id in its path, so no separate
 * `getSession()` call is needed (same reasoning both predecessor pages
 * already established). `getMyTickets()` is gated identically (also
 * `AuthenticatedUser`-only, also no id in its path), so a `null` from one
 * always means a `null` from the other in practice -- this page still
 * defensively falls back to `[]` for `tickets` rather than assuming that
 * invariant blindly, since the two are independent HTTP calls. */
export default async function MyTrackedTrainsPage() {
  const [trains, tickets] = await Promise.all([getMyTrackedTrains(), getMyTickets()]);

  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <LoginLink underline="always">
          Log in to see the trains and tickets you&apos;re tracking
        </LoginLink>
      </Stack>
    );
  }

  const ticketsByTrain = new Map<number, TicketListItem[]>();
  const unattachedTickets: TicketListItem[] = [];
  for (const ticket of tickets ?? []) {
    if (ticket.trackedTrainId === null) {
      unattachedTickets.push(ticket);
      continue;
    }
    const existing = ticketsByTrain.get(ticket.trackedTrainId) ?? [];
    existing.push(ticket);
    ticketsByTrain.set(ticket.trackedTrainId, existing);
  }

  const nothingToShow = trains.length === 0 && unattachedTickets.length === 0;

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <TextLink href="/track">Track a new train</TextLink>
      </Group>
      {nothingToShow ? (
        <Text c="dimmed">
          You haven&apos;t tracked any trains or added any tickets yet.{' '}
          <Link href="/track">Track a train</Link> to get started.
        </Text>
      ) : (
        <>
          {trains.length > 0 && (
            <Stack gap="xs">
              {trains.map((train) => (
                <TrackedTrainListRow key={train.id} train={train} tickets={ticketsByTrain.get(train.id) ?? []} />
              ))}
            </Stack>
          )}
          {unattachedTickets.length > 0 && (
            <Stack gap="md">
              <Title order={2}>Tickets not yet attached to a train</Title>
              <Text size="sm" c="dimmed">
                Extraction can&apos;t tell us exactly which service one of these tickets is for. Attach it to one of
                your tracked trains below, or track the right one.
              </Text>
              <Stack gap="lg">
                {unattachedTickets.map((ticket, index) => (
                  <Stack key={ticket.id} gap="xs">
                    {index > 0 && <Divider />}
                    <UnattachedTicketRow ticket={ticket} trains={trains} />
                  </Stack>
                ))}
              </Stack>
            </Stack>
          )}
        </>
      )}
      <TicketEntryForm label="Add a ticket" />
    </Stack>
  );
}

function TrackedTrainListRow({ train, tickets }: { train: TrackedTrainListItem; tickets: TicketListItem[] }) {
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

  const route = train.pinDestinationCrs ? `${train.pinOriginCrs} → ${train.pinDestinationCrs}` : train.pinOriginCrs;

  return (
    <Card withBorder>
      <Stack gap="sm">
        {/* Only the header row itself is the link -- attached tickets
            below render their own outbound Delay Repay link
            (DelayRepayEstimate), and nesting an <a> inside another <a>
            (wrapping the whole card, as the trains-only predecessor page
            did) is invalid HTML once that's a real possibility. */}
        <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
          <Stack gap={4}>
            <Group justify="space-between" wrap="nowrap">
              <Text fw={500}>{route}</Text>
              <RowStatusBadge train={train} />
            </Group>
            <Text size="sm" c="dimmed">
              {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
            </Text>
          </Stack>
        </Link>
        {tickets.length > 0 && (
          <Stack
            gap="md"
            pl="md"
            style={{ borderLeft: '2px solid var(--mantine-color-default-border)' }}
          >
            {tickets.map((ticket) => (
              <Stack key={ticket.id} gap={4}>
                <TicketSummary ticket={ticket} />
                {/* Imported and used exactly as-is, no new props, no
                    wrapper -- literal reuse of the already-reviewed
                    rendering, same as both predecessor pages. */}
                <DelayRepayEstimate
                  response={{
                    delayMinutes: ticket.delayMinutes,
                    estimate: ticket.estimate,
                    claimUrl: ticket.claimUrl,
                    disclaimer: ticket.disclaimer,
                  }}
                />
              </Stack>
            ))}
          </Stack>
        )}
      </Stack>
    </Card>
  );
}

function UnattachedTicketRow({ ticket, trains }: { ticket: TicketListItem; trains: TrackedTrainListItem[] }) {
  // Same "find or track the train this is for" mechanism
  // `TicketEntryForm`'s own post-save next step uses -- the ticket's
  // origin (if any) pre-fills `TrackTrainForm`, and its id is carried
  // forward so a newly-created pin attaches automatically.
  const trackParams = new URLSearchParams();
  if (ticket.originCrs) {
    trackParams.set('origin', ticket.originCrs);
  }
  trackParams.set('ticketId', String(ticket.id));

  return (
    <Card withBorder>
      <Stack gap="sm">
        <TicketSummary ticket={ticket} />
        <DelayRepayEstimate
          response={{
            delayMinutes: ticket.delayMinutes,
            estimate: ticket.estimate,
            claimUrl: ticket.claimUrl,
            disclaimer: ticket.disclaimer,
          }}
        />
        <Group gap="lg" wrap="wrap" align="flex-end">
          <AttachTicketAction ticketId={ticket.id} trains={trains} />
          <TextLink href={`/track?${trackParams.toString()}`} underline="always">
            Track a new train for this ticket
          </TextLink>
        </Group>
      </Stack>
    </Card>
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
