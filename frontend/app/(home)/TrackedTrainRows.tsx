import { Card, Group, Stack, Text } from '@mantine/core';
import { CardLink } from '@/components/CardLink';
import { StateBadge } from '@/components/StateBadge';
import { StatusBadge } from '@/components/StatusBadge';
import { StatusRow } from '@/components/StatusRow';
import { TrackedTrainStatusBadge } from '@/components/TrackedTrainStatusBadge';
import { formatDate, formatTime } from '@/lib/dateFormat';
import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from '@/lib/memberLabel';
import { trackedTrainRouteLabel } from '@/lib/trackingName';
import type { MergedSharedCustomLine } from '@/lib/sharedCustomLines';
import type { MergedSharedTrain } from '@/lib/sharedTrains';
import { worstStatus } from '@/lib/severity';
import { tokens } from '@/lib/theme';
import type { LineStatusReport, TrackedTrainListItem } from '@/lib/types';

/* The home page's tracked-train and shared-line rows. */

/** One custom line a fellow group member has shared with the caller.
 *
 * View-only by construction: the whole card is a link to `/lines/{lineId}`
 * and there is no pin toggle, no edit and no un-share control anywhere on
 * it. A grant conveys read access and nothing else -- only the line's
 * owner can change or delete it, and only the owner (or a group
 * `admin`/`owner`, from the group's own page) can stop the sharing.
 *
 * The "From {group}" `StateBadge` (action tone) is the same tag `/journeys` already
 * uses for a group-shared train, deliberately worded and coloured
 * identically so the two surfaces read as one convention. One badge per
 * group this line reached the caller through -- a line shared into two of
 * their groups is two tags, not an arbitrarily-picked one. */
export function SharedCustomLineSummaryRow({
  row,
  report,
}: {
  row: MergedSharedCustomLine;
  report: LineStatusReport | undefined;
}) {
  const { line, groupNames } = row;
  return (
    <CardLink href={`/lines/${line.lineId}`}>
      <Stack gap={4}>
        <StatusRow
          title={line.lineName}
          trailing={report && <StatusBadge severity={worstStatus(report).statusSeverity} />}
        />
        <Group gap="xs" wrap="wrap">
          {groupNames.map((groupName) => (
            <StateBadge key={groupName} tone="action">
              From {groupName}
            </StateBadge>
          ))}
          {/* `grantedByName` is null when the sharer has no name or
                username on their account -- never their email, which is
                not something to show the rest of a group
                (`crates/api/src/data/users.rs`'s `MemberDisplay`). Then
                "a member", suffixed with the opaque tag the backend sends
                in its place so an IdP that can name nobody still yields
                distinguishable rows -- never a raw user id, never an
                email. Same `memberLabel` helper the shared-train row below
                and `/groups/{id}` use, so none of them can drift. */}
          <Text size="sm" c="dimmed">
            Shared by {memberLabel(line.grantedByName, line.grantedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        </Group>
      </Stack>
    </CardLink>
  );
}

/** One row of the "Your Tracked Trains" section: either a train the caller
 * tracked themselves, or one another member shared into a group they
 * belong to. A discriminated union rather than one widened row type, so
 * the shared half can never accidentally be handed to the own-row
 * component (which links to the owner-scoped `/train/by-id/{id}` route) or
 * vice versa. */
export type TrackedTrainRow = { kind: 'own'; train: TrackedTrainListItem } | { kind: 'shared'; row: MergedSharedTrain };

/** A train another member shared into a group the caller belongs to,
 * rendered in the same list as the caller's own summary rows above.
 * Home-page-local sibling of `TrackedTrainSummaryRow` below, and a trimmed
 * mirror of /journeys's own `SharedTrainListRow` -- same tags, same
 * wording, same linking rule.
 *
 * Deliberately NOT `TrackedTrainSummaryRow` with extra props: the caller
 * doesn't own this train, so none of an own row's affordances apply. There
 * is no rename (`POST /Train/{id}/name` is owner-scoped), no ticket data
 * (spec §4 forbids a shared train ever carrying any) and no delete -- and
 * the home page's own rows carry none of those controls either, so the
 * distinction that actually matters here is the LINK: an own row falls back
 * to `/train/by-id/{id}`, which is owner-scoped and 404s for anyone else.
 *
 * So this row links exactly when a `trainUid` is known, and renders plain
 * text otherwise -- a deliberately weaker test than the own row's
 * `resolutionStatus === 'resolved' && trainUid`, because `trainUid` is
 * populated well before the status reaches `resolved` and
 * `/train/{uid}/{date}` is public and unscoped. Same reasoning
 * /journeys's shared row states at length. */
export function SharedTrainSummaryRow({ row }: { row: MergedSharedTrain }) {
  const { train, groupNames } = row;
  // `routeLabel`, not `trackedTrainDisplayName`: this page's own rows
  // (`TrackedTrainSummaryRow` below) label by route too, so using the
  // sharer's `customName` here would make the two halves of one list
  // disagree about what a row's heading even is. /journeys, whose own
  // rows DO show a custom name, shows it for shared rows for the same
  // consistency reason.
  const route = trackedTrainRouteLabel(train);
  // Same date-only degradation as the own row below, for the same reason:
  // a pin with no schedule data yet has no departure time, and `Invalid
  // Date` is never an acceptable label.
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  const href = train.trainUid ? `/train/${train.trainUid}/${train.serviceDate}` : null;

  const content = (
    <Stack gap={4}>
      <StatusRow title={route} trailing={<TrackedTrainStatusBadge train={train} />} />
      <Text size="sm" c="dimmed">
        {when}
      </Text>
      <Group gap="xs" wrap="wrap">
        {/* One badge per group this train reached the caller through --
              a train shared into two of their groups is two tags, not an
              arbitrarily-picked one. `addedByName` is `null` when the
              sharer has neither a name nor a username on their account --
              never their email, which is not something to show the rest of
              a group (`crates/api/src/data/users.rs`'s `display_label`).
              "a member" then, never a raw user id, suffixed with the
              sharer's `addedByTag` so two such sharers don't read
              identically -- same helper and same wording /journeys and
              /groups/{id} already use (`lib/memberLabel.ts`). */}
        {groupNames.map((groupName) => (
          <StateBadge key={groupName} tone="action">
            From {groupName}
          </StateBadge>
        ))}
        <Text size="sm" c="dimmed">
          Shared by {memberLabel(train.addedByName, train.addedByTag, MEMBER_PLACEHOLDER_INLINE)}
        </Text>
      </Group>
    </Stack>
  );

  // Whole-card link when there's somewhere to go, matching the own row
  // directly below; nothing inside the card is itself interactive, so
  // there's no nested-<a> problem of the kind /journeys's richer row has
  // to work around.
  return href ? <CardLink href={href}>{content}</CardLink> : <Card {...tokens.card.static}>{content}</Card>;
}

// Home-page-local mirror of /journeys's own row shape
// (frontend/app/journeys/page.tsx's TrackedTrainListRow) -- same fields,
// same resolutionStatus-vs-status+delayMinutes branching. The badge itself
// is shared (`TrackedTrainStatusBadge`, review §2.9) rather than
// re-derived per page: both this row and /groups/[id]'s shared-train card
// render off the same component now, so the two pages can't drift on
// status wording the way they used to.
export function TrackedTrainSummaryRow({ train }: { train: TrackedTrainListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- same logic as /journeys's own row. The
  // resolved-with-null-trainUid fallback is defensive: the backend's own
  // resolution invariant means this shouldn't happen, but this component
  // doesn't assume it.
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = trackedTrainRouteLabel(train);

  return (
    <CardLink href={href}>
      <Stack gap={4}>
        <StatusRow title={route} trailing={<TrackedTrainStatusBadge train={train} />} />
        <Text size="sm" c="dimmed">
          {/* `pinScheduledDeparture` is `null` for an NR-primary
                subscription whose train has no schedule data yet -- same
                date-only degradation as /journeys's own row. */}
          {train.pinScheduledDeparture
            ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
            : formatDate(train.serviceDate)}
        </Text>
      </Stack>
    </CardLink>
  );
}
