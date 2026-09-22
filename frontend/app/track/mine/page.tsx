import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTrackedTrains, getMyTickets, getSharedGroupTrains, getMyJourneys } from '@/lib/api';
import { AutoOpenLoginPrompt } from './AutoOpenLoginPrompt';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { TicketSummary } from '@/components/TicketSummary';
import { ReliabilityDigest } from '@/components/ReliabilityDigest';
import { DelayRepayEstimate } from '@/components/DelayRepayEstimate';
import { AttachTicketAction } from '@/components/AttachTicketAction';
import { DeleteTicketButton } from '@/components/DeleteTicketButton';
import { RenameTicketButton } from '@/components/RenameTicketButton';
import { StatusRow } from '@/components/StatusRow';
import { TrackedTrainRowMenu } from '@/components/TrackedTrainRowMenu';
import { TrackedTrainStatusBadge } from '@/components/TrackedTrainStatusBadge';
import { formatDate, formatTime } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import { mergeSharedTrains, type MergedSharedTrain } from '@/lib/sharedTrains';
import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from '@/lib/memberLabel';
import { JourneyStatusGroupBadge } from '@/components/JourneyStatusBadge';
import { journeyListItemStatusGroup } from '@/lib/journeyStatus';
import type { TrackedTrainListItem, TicketListItem, JourneyListItem } from '@/lib/types';

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
 * invariant blindly, since the two are independent HTTP calls.
 *
 * `getSharedGroupTrains()` is the third call, gated and null-on-401 in
 * exactly the same way, and is what makes this "my trains" rather than
 * "trains I personally pressed Track on": a train someone shared into a
 * group the caller belongs to belongs in this list too, tagged with where
 * it came from. Before this, sharing a train had no effect whatsoever on
 * the recipient's own tracked-trains page -- the only place a shared train
 * appeared at all was `/groups/{id}`, which a member had to already think
 * to open.
 *
 * `.catch(() => null)` on that third call alone: unlike the other two, it
 * is AUXILIARY to this page -- the caller's own trains and tickets are
 * what the page is for, and losing the group-shared half for the duration
 * of a backend hiccup is materially better than losing the whole page to
 * the error boundary, the same trade-off `app/page.tsx` already states for
 * its own non-essential fetches. `null` is a value this page already
 * handles (it's `getSharedGroupTrains()`'s own 401 return), so the failure
 * collapses into the existing "nothing shared with you" branch rather than
 * needing one of its own.
 *
 * `getMyJourneys()` is the fourth call, added for the 2026-09-22 UX
 * review's C1. Before it, a journey created by time-window search was
 * unreachable the moment the user left its page: this list was built from
 * `trains`/`shared` alone, an unmatched leg has
 * `train_subscription_id IS NULL` and so could never produce a row here,
 * `navLinks.ts` has no journey entry, and the journey page itself is
 * reached only by a `router.push`. Every other private object in this app
 * -- trains, tickets, groups, custom lines -- has a list page; the one
 * whose whole status is "needs you to come back and pick a train" did not.
 * Gated and null-on-401 exactly like the first two, and `.catch(() => null)`
 * for the same "auxiliary, don't lose the whole page over it" reason as
 * `getSharedGroupTrains()`. */
export default async function MyTrackedTrainsPage() {
  const [trains, tickets, sharedTrains, journeys] = await Promise.all([
    getMyTrackedTrains(),
    getMyTickets(),
    getSharedGroupTrains().catch(() => null),
    getMyJourneys().catch(() => null),
  ]);

  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Trains &amp; Tickets</Title>
        {/* Server-rendered, same pattern as
            app/train/by-id/[trackingId]/page.tsx's own
            ApiUnauthorizedError branch: a link-unfurler bot or a
            pre-hydration visitor sees this sentence even though it can
            never run the client-only AutoOpenLoginPrompt modal below,
            which stays as progressive enhancement on top of it. */}
        <LoginLink underline="always">
          Log in to see the trains and tickets you&apos;re tracking
        </LoginLink>
        <AutoOpenLoginPrompt>
          Log in to see the trains and tickets you&apos;re tracking.
        </AutoOpenLoginPrompt>
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

  // De-duplicated per train, and filtered against the caller's own rows --
  // see `mergeSharedTrains`' own doc comment for why both halves of that
  // are the frontend's job rather than the query's.
  const shared = mergeSharedTrains(sharedTrains ?? [], new Set(trains.map((t) => t.id)));

  // Split deliberately. `hasOwnContent` is the OLD `!nothingToShow`, and
  // still gates the reliability digest alone: that card is the caller's
  // own punctuality/Delay Repay record ("Your reliability"), computed from
  // their own trains and tickets, so a shared train is not evidence about
  // it and must not make an otherwise-empty digest appear.
  //
  // `nothingToShow` now also accounts for shared trains: a member who
  // tracks nothing themselves but has trains shared with them has
  // something to show, and the empty state ("you haven't tracked any
  // trains") would be both wrong and -- since it's the branch that hides
  // the list entirely -- the very bug this page had.
  const journeyRows = journeys ?? [];
  // A journey's CURRENT leg, once matched, owns a real `train_subscriptions`
  // row -- which is also one of this user's own tracked trains, so
  // `getMyTrackedTrains()` returns it too. Showing both would put the same
  // service on screen twice under two different nouns, which is exactly
  // the confusion the review's I22 names. The journey row wins: it links
  // to `/journeys/{id}` (the page the user was shown when they created it)
  // and it knows about the journey's other legs, which the bare train row
  // does not.
  //
  // HONEST LIMIT: `GET /Journeys/mine` surfaces only the current leg's
  // `trainSubscriptionId`, so an EARLIER, already-completed leg's train
  // still appears as its own row. That is a degraded case, not a wrong
  // one -- a completed train genuinely is a tracked train -- and closing
  // it properly means returning every leg's subscription id from that
  // endpoint, which belongs with the wider I22 vocabulary work rather
  // than here.
  const journeyTrainIds = new Set(
    journeyRows
      .map((journey) => journey.trainSubscriptionId)
      .filter((id): id is number => id !== null),
  );
  const standaloneTrains = trains.filter((train) => !journeyTrainIds.has(train.id));

  // `hasOwnContent` still gates the reliability digest ALONE and so still
  // counts only trains and tickets -- a journey with no train picked yet is
  // not evidence about the caller's punctuality record, and its matched
  // legs are already counted via `trains`. `nothingToShow` does count
  // journeys: a user whose only tracked object is an unmatched journey has
  // something to show, and the "you haven't tracked any trains" empty
  // state would both be wrong and (since it's the branch that hides the
  // list) hide the very row C1 exists to add.
  const hasOwnContent = trains.length > 0 || unattachedTickets.length > 0;
  const nothingToShow = !hasOwnContent && shared.length === 0 && journeyRows.length === 0;

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <Group gap="md">
          <TextLink href="/track">Track a new train</TextLink>
          <TextLink href="/track/mine/add-ticket">Add a ticket</TextLink>
        </Group>
      </Group>
      {hasOwnContent && <ReliabilityDigest trains={trains} tickets={tickets ?? []} />}
      {nothingToShow ? (
        <Text c="dimmed">
          You haven&apos;t tracked any trains or added any tickets yet.{' '}
          <Link href="/track">Track a train</Link> to get started.
        </Text>
      ) : (
        <>
          {journeyRows.length > 0 && (
            <Stack gap="xs">
              <Title order={2}>Your journeys</Title>
              {journeyRows.map((journey) => (
                <JourneyListRow key={journey.id} journey={journey} />
              ))}
            </Stack>
          )}
          {(standaloneTrains.length > 0 || shared.length > 0) && (
            // ONE list, not a "shared with me" section of its own: the
            // whole point of the fix is that a shared train sits alongside
            // the caller's own, which is also why each shared row carries
            // its "from <group>"/"Shared by <who>" tags -- in a merged
            // list, a row with no attribution would read as one the caller
            // tracked themselves.
            //
            // Own rows first, shared rows after, each half in the order
            // its own endpoint returned. Interleaving would mean re-sorting
            // the caller's own half on something other than `trackedAt`,
            // and that ordering is a deliberate, reasoned choice of its own
            // (`list_tracked_trains_for_user`'s doc comment: a train pinned
            // a month out must not outrank one pinned five minutes ago for
            // a service running right now). The obvious shared key --
            // `serviceDate`/`pinScheduledDeparture` -- would override
            // exactly that, and `trackedAt` itself can't be the merge key
            // because a shared train deliberately never exposes one (spec
            // §4's "Never shown" list). So: two halves, each honestly
            // ordered, rather than one list ordered by something neither
            // half chose.
            <Stack gap="xs">
              {standaloneTrains.map((train) => (
                <TrackedTrainListRow key={train.id} train={train} tickets={ticketsByTrain.get(train.id) ?? []} />
              ))}
              {shared.map((row) => (
                <SharedTrainListRow key={row.train.trainSubscriptionId} row={row} />
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
    </Stack>
  );
}

/** One row of `GET /Journeys/mine`, linking to `/journeys/{id}` -- the
 * page the user was pushed to when they created it, and until this row
 * existed the only way back to it (2026-09-22 UX review, C1).
 *
 * Route and date go through `routeLabel`/`formatDate`, the same two
 * helpers the tracked-train rows below already use: a journey row printing
 * "KGX → YRK, 2026-09-22" directly above a train row printing "London
 * Kings Cross (KGX) → York (YRK), 22 Sept 2026" would be a fresh instance
 * of the review's own P5 in the one list where both formats are visible at
 * once. `GET /Journeys/mine` grew `serviceDate`/`originName`/
 * `destinationName` for exactly this.
 *
 * Built on `StatusRow` rather than a hand-rolled
 * `Group justify="space-between"`, for the shrink-safe title/trailing
 * behaviour the primitive encodes (review P2) -- the same reason
 * `ScheduleRow` is built on it. */
function JourneyListRow({ journey }: { journey: JourneyListItem }) {
  const route = routeLabel(
    journey.originCrs,
    journey.originName,
    journey.destinationCrs,
    journey.destinationName,
  );
  const when = formatDate(journey.serviceDate);
  const title = journey.customName ?? `${route}, ${when}`;

  return (
    <Card withBorder>
      <Stack gap={4}>
        <StatusRow
          align="flex-start"
          title={
            <Link href={`/journeys/${journey.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
              <Text fw={500} lineClamp={2} style={{ minWidth: 0 }}>
                {title}
              </Text>
            </Link>
          }
          trailing={<JourneyStatusGroupBadge group={journeyListItemStatusGroup(journey)} />}
        />
        {/* Only when a custom name has replaced the default title -- the
            same rule the tracked-train rows below follow, so the route and
            date are never printed twice on one row. */}
        {journey.customName && (
          <Text size="sm" c="dimmed">
            {route}, {when}
          </Text>
        )}
      </Stack>
    </Card>
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

  const route = routeLabel(
    train.pinOriginCrs,
    train.pinOriginName,
    train.pinDestinationCrs,
    train.pinDestinationName,
  );
  const displayName = trackedTrainDisplayName(train);
  // `pinScheduledDeparture` is `null` for an NR-primary subscription whose
  // train has no schedule data yet -- degrade to a date-only label rather
  // than rendering `Invalid Date`. Kept identical to
  // `trackedTrainDisplayName`'s own fallback so the rename dialog's
  // placeholder always matches the label it would replace.
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  const defaultName = `${route}, ${when}`;

  return (
    <Card withBorder>
      <Stack gap="sm">
        {/* Only the header row itself is the link -- attached tickets
            below render their own outbound Delay Repay link
            (DelayRepayEstimate), and nesting an <a> inside another <a>
            (wrapping the whole card, as the trains-only predecessor page
            did) is invalid HTML once that's a real possibility.
            RenameTrainButton sits outside the <Link> for the same reason. */}
        <StatusRow
          align="flex-start"
          title={
            <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
              <Stack gap={4}>
                <StatusRow title={displayName} trailing={<TrackedTrainStatusBadge train={train} />} />
                {/* Task 3.6.8: `displayName` (`trackedTrainDisplayName`)
                    already falls back to `${route}, ${when}` -- the exact
                    same `when` string -- whenever no custom name is set,
                    so printing this dimmed line unconditionally repeated
                    the date/time on screen twice for every train without
                    one. It's only new information once a custom name has
                    replaced the default title above. */}
                {train.customName && (
                  <Text size="sm" c="dimmed">
                    {when}
                  </Text>
                )}
              </Stack>
            </Link>
          }
          trailing={
            // Task 3.6.7: one overflow kebab instead of a standalone
            // "Rename" button with no "stop tracking" affordance at all on
            // this row (that action previously only existed on the detail
            // page) -- also relieves Task 1.5's space contest on this same
            // trailing slot, which otherwise stacks the status badge
            // alongside an ever-growing set of per-row controls.
            //
            // The Menu/trigger composition itself lives in
            // TrackedTrainRowMenu (a Client Component): this page is a
            // Server Component, and RenameTrainButton/DeleteTrainButton's
            // `trigger` prop is a function -- functions cannot cross the
            // Server->Client boundary as a prop, only serializable values
            // can, so the closures that build each Menu.Item have to be
            // constructed client-side, not passed in from here.
            <TrackedTrainRowMenu
              displayName={displayName}
              trackingId={train.id}
              customName={train.customName}
              defaultName={defaultName}
              sharedGroupCount={train.sharedGroupCount}
            />
          }
        />
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
                <Group gap="xs">
                  <RenameTicketButton
                    ticketId={ticket.id}
                    customName={ticket.customName}
                    defaultName={`${ticket.operator ?? 'Ticket'}${ticket.ticketType ? ` — ${ticket.ticketType}` : ''}`}
                  />
                  <DeleteTicketButton ticketId={ticket.id} />
                </Group>
              </Stack>
            ))}
          </Stack>
        )}
      </Stack>
    </Card>
  );
}

/** A train another member shared into a group the caller belongs to,
 * rendered in the same list as the caller's own rows above.
 *
 * Deliberately NOT a `TrackedTrainListRow` with extra props: none of that
 * row's controls apply to a train the caller doesn't own. There's no
 * rename (`POST /Train/{id}/name` is owner-scoped), no ticket sub-list
 * (spec §4 forbids a shared train ever carrying ticket data at all), and
 * no delete. What's left in common -- the display name, the when line, the
 * status/delay badges -- is shared directly (`trackedTrainDisplayName`,
 * `TrackedTrainStatusBadge`) rather than duplicated.
 *
 * The header links exactly when a `trainUid` is known, and not otherwise.
 * `/train/[uid]/[date]` is public and unscoped, so a uid is the whole
 * precondition -- deliberately a WEAKER test than the own-row's
 * `resolutionStatus === 'resolved' && trainUid`, because `trains_id` (and
 * so `trainUid`) is populated well before the status reaches `resolved`
 * (`schedule_matched`, and an NR-primary subscription created by
 * `create_subscription_for_train`, both have one while still short of
 * `resolved`). The own row can afford the stricter test because it falls
 * back to `/train/by-id/{id}`; this row can't, since that route
 * (`GET /Train/{id}`) is owner-scoped and 404s for everyone else -- which
 * is also why a uid-less shared train is rendered as plain text here
 * rather than linked, the same dead-end reasoning that leaves
 * `/groups/{id}`'s own shared rows unlinked entirely. */
function SharedTrainListRow({ row }: { row: MergedSharedTrain }) {
  const { train, groupNames } = row;
  const displayName = trackedTrainDisplayName({
    customName: train.customName,
    pinOriginCrs: train.pinOriginCrs,
    pinOriginName: train.pinOriginName,
    pinDestinationCrs: train.pinDestinationCrs,
    pinDestinationName: train.pinDestinationName,
    serviceDate: train.serviceDate,
    pinScheduledDeparture: train.pinScheduledDeparture,
  });
  // Same date-only degradation as the own-train row directly above, for
  // the same reason (a pin with no schedule data yet has no departure
  // time, and `Invalid Date` is never an acceptable label).
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  const href = train.trainUid ? `/train/${train.trainUid}/${train.serviceDate}` : null;
  const heading = (
    <Text fw={500} lineClamp={2} style={{ minWidth: 0 }}>
      {displayName}
    </Text>
  );

  return (
    <Card withBorder>
      <Stack gap={4}>
        <StatusRow
          align="flex-start"
          title={
            href ? (
              <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
                {heading}
              </Link>
            ) : (
              heading
            )
          }
          trailing={<TrackedTrainStatusBadge train={train} />}
        />
        {/* Task 3.6.8: same fix as the caller's own row above -- `heading`
            already falls back to `${route}, ${when}` whenever this
            sharer never set a custom name, so print the dimmed line only
            when a custom name replaced it in the heading. */}
        {train.customName && (
          <Text size="sm" c="dimmed">
            {when}
          </Text>
        )}
        <Group gap="xs" wrap="wrap">
          {/* One badge per group this train reached the caller through --
              a train shared into two of their groups is two tags, not an
              arbitrarily-picked one. `addedByName` is `null` when the
              sharer has neither a name nor a username on their account --
              never their email, which is not something to show the rest
              of a group (`crates/api/src/data/users.rs`'s
              `display_label`). "a member" then, never a raw user id,
              suffixed with the sharer's `addedByTag` so two such sharers
              don't read identically -- same helper and same wording as
              `/groups/{id}`'s shared rows (`lib/memberLabel.ts`). */}
          {groupNames.map((groupName) => (
            <Badge key={groupName} variant="light" color="grape">
              from {groupName}
            </Badge>
          ))}
          <Text size="sm" c="dimmed">
            Shared by {memberLabel(train.addedByName, train.addedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        </Group>
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
          <RenameTicketButton
            ticketId={ticket.id}
            customName={ticket.customName}
            defaultName={`${ticket.operator ?? 'Ticket'}${ticket.ticketType ? ` — ${ticket.ticketType}` : ''}`}
          />
          <DeleteTicketButton ticketId={ticket.id} />
        </Group>
      </Stack>
    </Card>
  );
}

