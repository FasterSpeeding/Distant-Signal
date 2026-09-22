import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import {
  getGroup,
  getGroupCustomLines,
  getGroupJourneys,
  getGroupMembers,
  getGroupTrains,
  getLineStatus,
  getSession,
  ApiNotFoundError,
  ApiUnauthorizedError,
} from '@/lib/api';
import { RemoveMemberButton } from '@/components/RemoveMemberButton';
import { PromoteMemberButton } from '@/components/PromoteMemberButton';
import { DemoteMemberButton } from '@/components/DemoteMemberButton';
import { LeaveGroupButton } from '@/components/LeaveGroupButton';
import { RenameGroupButton } from '@/components/RenameGroupButton';
import { DeleteGroupButton } from '@/components/DeleteGroupButton';
import { GroupInviteLinkCard } from '@/components/GroupInviteLinkCard';
import { RemoveGroupTrainButton } from '@/components/RemoveGroupTrainButton';
import { AddTrainToGroupButton } from '@/components/AddTrainToGroupButton';
import { AddCustomLineToGroupButton } from '@/components/AddCustomLineToGroupButton';
import { RemoveCustomLineGrantButton } from '@/components/RemoveCustomLineGrantButton';
import { AddJourneyToGroupButton } from '@/components/AddJourneyToGroupButton';
import { RemoveGroupJourneyButton } from '@/components/RemoveGroupJourneyButton';
import { StatusBadge } from '@/components/StatusBadge';
import { StatusRow } from '@/components/StatusRow';
import { TrackedTrainStatusBadge } from '@/components/TrackedTrainStatusBadge';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import { worstStatus } from '@/lib/severity';
import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from '@/lib/memberLabel';
import { getSiteOrigin } from '@/lib/siteOrigin';
import type { GroupCustomLine, GroupJourney, GroupMember, GroupTrain, LineStatusReport } from '@/lib/types';

export const revalidate = 0;

/** `/groups/{id}` -- group detail: the group's own header controls
 * (rename/delete/leave), members (with role badges, remove/promote/
 * invite-link), and the trains shared into the group.
 *
 * Every control here is gated on the caller's role using the SAME
 * predicate the corresponding backend handler uses
 * (`crates/api/src/routes/groups.rs`), not a coarser one -- `canManage`
 * (`admin`/`owner`) for rename, member removal, and the invite-link card;
 * `viewerIsOwner` for promotion, demotion and group deletion;
 * sharer-or-manager for
 * un-sharing a train. This is presentational only -- the backend is still
 * the authority and refuses any of these regardless -- but showing a user
 * a button whose only possible outcome is a 403/404 is its own bug. */
export default async function GroupDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;

  let group;
  try {
    [group] = await Promise.all([getGroup(id)]);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Group not found</Title>
          <Text c="dimmed">This group doesn&apos;t exist, or you&apos;re not a member of it.</Text>
        </Stack>
      );
    }
    // Same reasoning as `app/train/by-id/[trackingId]/page.tsx`'s own
    // `ApiUnauthorizedError` branch: a lapsed session on a page with no
    // public sibling content deserves a "log in, this might be yours"
    // prompt rather than falling through to the generic `app/error.tsx`.
    // Checked before the rethrow so only genuinely unexpected errors reach
    // the error boundary.
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Group</Title>
          <LoginLink underline="always">Log in to view this group</LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  const [members, trains, customLines, journeys, session] = await Promise.all([
    getGroupMembers(id),
    getGroupTrains(id),
    getGroupCustomLines(id),
    getGroupJourneys(id),
    getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null })),
  ]);

  // Live status for the shared custom lines, read through the ordinary
  // `/Line/{ids}/Status` route rather than a second, parallel status path
  // baked into `GET /groups/{id}/lines/custom`. That route is exactly the
  // one a grant widens, so this is also the end-to-end proof the widening
  // works for this viewer.
  //
  // Only `ApiNotFoundError` is swallowed, not every failure: that route
  // 404s when it matches nothing at all (a line the aggregator hasn't
  // computed a status for yet), which is an ordinary state here and should
  // degrade to "no badge". A 401, a 5xx or a dead socket is not, and
  // silently rendering every shared line as statusless would hide it.
  let customLineReports: LineStatusReport[] = [];
  if (customLines.length > 0) {
    customLineReports = await getLineStatus(
      customLines.map((l) => l.lineId),
      false,
    ).catch((err) => {
      if (err instanceof ApiNotFoundError) return [];
      throw err;
    });
  }
  const reportByLineId = new Map(customLineReports.map((r) => [r.id, r]));
  const currentUserId = session.authenticated ? session.id : null;
  const canManage = group.role === 'owner' || group.role === 'admin';
  // Distinct from `canManage`: the backend gates promotion, demotion and
  // group deletion on `GroupRole::is_owner`, NOT `can_manage`
  // (`crates/api/src/routes/groups.rs`), so showing any of those controls
  // to an admin would be offering a button whose only outcome is a 403.
  const viewerIsOwner = group.role === 'owner';
  const willDeleteGroup = viewerIsOwner && group.memberCount === 1;
  // Who inherits ownership if the current viewer (the owner) leaves and the
  // group ISN'T deleted outright -- see `LeaveGroupButton`'s own doc
  // comment. Mirrors `remove_member`'s own successor query
  // (`crates/api/src/data/groups.rs`) exactly: the longest-standing
  // remaining admin, or failing that the longest-standing remaining member.
  // Only ever computed (and only ever passed down) for an owner who isn't
  // the sole member -- every other viewer gets `null` and the generic copy.
  const nextOwner =
    viewerIsOwner && !willDeleteGroup
      ? [...members]
          .filter((m) => m.userId !== currentUserId)
          .sort((a, b) => {
            const roleRank = (m: GroupMember) => (m.role === 'admin' ? 0 : 1);
            const rankDiff = roleRank(a) - roleRank(b);
            return rankDiff !== 0 ? rankDiff : a.joinedAt.localeCompare(b.joinedAt);
          })[0]
      : undefined;
  const nextOwnerLabel = nextOwner ? memberLabel(nextOwner.displayName, nextOwner.displayTag) : null;
  // Only actually used by GroupInviteLinkCard below (canManage-gated), but
  // resolved unconditionally rather than behind an `if (canManage)` --
  // it's a cheap header/env read, and keeping it unconditional means this
  // call site can't silently start passing a stale/undefined origin if a
  // future edit reorders things around the `canManage` check.
  const origin = await getSiteOrigin();

  return (
    <Stack p="lg" gap="lg">
      {/* Review §3.2.5: the detail page had no way back to `/groups` short
          of the browser's own Back button (which fails outright for a
          visitor who followed a deep link straight here). Mirrors
          `app/lines/[id]/history/page.tsx`'s own "Back to line" TextLink,
          placed above the `<h1>` the same way. */}
      <TextLink href="/groups" underline="always">
        ← Groups
      </TextLink>
      <Group justify="space-between" align="baseline">
        <Title order={1}>{group.name}</Title>
        {/* Review §3.2.1 (design decision): "Delete group" used to sit
            here too, in the same red-outline style as "Leave group" --
            visually identical despite wildly different blast radii (one
            member's own access vs. the whole group, every member, gone).
            It now lives alone in the "Danger zone" section at the foot of
            the page, subtly styled, so the one button every viewer might
            actually want keeps the visual weight here.
            `groupHeaderActions` (app/globals.css) stacks these full-width
            with more breathing room between them below `xs`, where they
            used to sit 12px apart directly against the heading. */}
        <Group gap="xs" className="groupHeaderActions">
          {canManage && <RenameGroupButton groupId={id} currentName={group.name} />}
          {currentUserId && (
            <LeaveGroupButton
              groupId={id}
              currentUserId={currentUserId}
              willDeleteGroup={willDeleteGroup}
              nextOwnerLabel={nextOwnerLabel}
            />
          )}
        </Group>
      </Group>

      <Stack gap="sm">
        {/* `size="h4"` (review §3.2.5), matching `/lines/[id]`'s own
            section headings: an `order={2}` `Title` with no `size` renders
            at near-`h1` scale on mobile, which reads as a second page
            title rather than a subsection heading. The semantic level
            (`order={2}`, still correct for a heading one below the page's
            own `<h1>`) is unchanged -- this only shrinks the rendered
            size. */}
        <Title order={2} size="h4">
          Members
        </Title>
        {members.map((member) => (
          <MemberRow
            key={member.userId}
            groupId={id}
            member={member}
            canManage={canManage}
            viewerIsOwner={viewerIsOwner}
            currentUserId={currentUserId}
          />
        ))}
        {canManage && <GroupInviteLinkCard groupId={id} inviteLink={group.inviteLink} origin={origin} />}
      </Stack>

      <Divider />

      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={2} size="h4">
            Shared trains
          </Title>
          <AddTrainToGroupButton
            groupId={id}
            excludeTrainSubscriptionIds={trains.map((t) => t.trainSubscriptionId)}
          />
        </Group>
        {trains.length === 0 ? (
          <Text c="dimmed">No trains have been shared into this group yet.</Text>
        ) : (
          trains.map((train) => (
            <SharedTrainRow
              key={train.trainSubscriptionId}
              groupId={id}
              train={train}
              canManage={canManage}
              currentUserId={currentUserId}
            />
          ))
        )}
      </Stack>

      <Divider />

      {/* A fourth, separate section rather than folding custom lines into
          "Shared trains" (or into a future catalogue-line section): the
          add-affordance is genuinely different -- only a line's own OWNER
          can share it, unlike a public catalogue line anyone could add --
          and a merged list would have to explain per row why some entries
          can be added by anyone and others only by one specific person.
          See the design doc §3.4. */}
      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={2} size="h4">
            Shared custom lines
          </Title>
          <AddCustomLineToGroupButton
            groupId={id}
            excludeLineIds={customLines.map((l) => l.lineId)}
          />
        </Group>
        {customLines.length === 0 ? (
          <Text c="dimmed">No custom lines have been shared into this group yet.</Text>
        ) : (
          customLines.map((line) => (
            <SharedCustomLineRow
              key={line.lineId}
              groupId={id}
              line={line}
              report={reportByLineId.get(line.lineId)}
              canManage={canManage}
              currentUserId={currentUserId}
            />
          ))
        )}
      </Stack>

      <Divider />

      {/* A fifth, separate section, mirroring "Shared trains"'s own shape
          exactly (unlike "Shared custom lines", which has a genuinely
          different add-permission story) -- sharing a journey has the
          identical "any member may share one of THEIR OWN" model
          group_trains already established. See the journey-tracking
          spec's §6. */}
      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={2} size="h4">
            Shared journeys
          </Title>
          <AddJourneyToGroupButton
            groupId={id}
            excludeJourneyIds={journeys.map((j) => j.journeyId)}
          />
        </Group>
        {journeys.length === 0 ? (
          <Text c="dimmed">No journeys have been shared into this group yet.</Text>
        ) : (
          journeys.map((journey) => (
            <SharedJourneyRow
              key={journey.journeyId}
              groupId={id}
              journey={journey}
              canManage={canManage}
              currentUserId={currentUserId}
            />
          ))
        )}
      </Stack>

      {/* "Danger zone" (review §3.2.1, design decision) -- the demoted
          home for "Delete group", now a subtle red text button rather than
          a second red-outline button beside "Leave group" in the header.
          Owner-only, matching the button's own gating; the section itself
          doesn't render at all for anyone else, since there is nothing
          else in it. */}
      {viewerIsOwner && (
        <>
          <Divider />
          <Stack gap="sm">
            <Title order={2} size="h4">
              Danger zone
            </Title>
            <DeleteGroupButton groupId={id} name={group.name} />
          </Stack>
        </>
      )}
    </Stack>
  );
}

function MemberRow({
  groupId,
  member,
  canManage,
  viewerIsOwner,
  currentUserId,
}: {
  groupId: string;
  member: GroupMember;
  canManage: boolean;
  viewerIsOwner: boolean;
  currentUserId: string | null;
}) {
  // `memberLabel`, not a bare `displayName`: a member whose identity
  // provider has no name on file for them (or only an email-shaped one,
  // which the backend declines to show) renders as the generic
  // placeholder, suffixed with their `displayTag` so several such members
  // are still separate rows rather than one repeated "A member". See
  // `lib/memberLabel.ts`.
  const label = memberLabel(member.displayName, member.displayTag);
  const isOwner = member.role === 'owner';
  const isCurrentUser = currentUserId !== null && member.userId === currentUserId;
  return (
    // Plain `div`s with `app/globals.css` classes, not nested Mantine
    // `Group`s with responsive props -- `.groupMemberRow` stacks the
    // identity half above the actions half below `xs` (review §3.2.4:
    // the two ~36px action buttons used to wrap directly on top of each
    // other, crushed against the name, rather than getting their own
    // line).
    <div className="groupMemberRow">
      <Group gap="xs" wrap="nowrap">
        <Text>{label}</Text>
        {/* Review §3.2.5: with no name/identity marker of its own, nothing
            on this page told a viewer which row was THEM -- especially
            awkward for a placeholder-rendered member ("A member
            (#a1b2c3)") trying to match their own tag against the list. */}
        {isCurrentUser && (
          <Text c="dimmed" size="sm">
            (you)
          </Text>
        )}
        <Badge variant="outline">{member.role}</Badge>
      </Group>
      <Group gap="xs" wrap="nowrap" className="groupMemberRow__actions">
        {/* `viewerIsOwner`, not `canManage`: `promote_member` and
            `demote_member` are both gated on `GroupRole::is_owner`
            server-side, so an admin who clicked either would only ever get
            a 403. The role check picks the one control that can actually
            do something for this row: a plain member can only be promoted,
            an admin can only be demoted, and the owner's own row gets
            neither (they're a permanent owner -- the backend refuses both
            directions against it). */}
        {viewerIsOwner && member.role === 'member' && <PromoteMemberButton groupId={groupId} userId={member.userId} />}
        {viewerIsOwner && member.role === 'admin' && (
          <DemoteMemberButton groupId={groupId} userId={member.userId} name={label} />
        )}
        {canManage && !isOwner && <RemoveMemberButton groupId={groupId} userId={member.userId} name={label} />}
      </Group>
    </div>
  );
}

function SharedTrainRow({
  groupId,
  train,
  canManage,
  currentUserId,
}: {
  groupId: string;
  train: GroupTrain;
  canManage: boolean;
  currentUserId: string | null;
}) {
  // Mirrors `groups::remove_train_from_group`'s own sharer-or-manager
  // check: a plain member may only un-share a train THEY shared. Rendering
  // this for every row meant a plain member saw "Remove from group" on
  // someone else's train and got a 404 for clicking it.
  const canRemove = canManage || (currentUserId !== null && train.addedBy === currentUserId);
  const displayName = trackedTrainDisplayName({
    customName: train.customName,
    pinOriginCrs: train.pinOriginCrs,
    pinOriginName: train.pinOriginName,
    pinDestinationCrs: train.pinDestinationCrs,
    pinDestinationName: train.pinDestinationName,
    serviceDate: train.serviceDate,
    pinScheduledDeparture: train.pinScheduledDeparture,
  });
  return (
    <Card withBorder>
      <StatusRow
        title={displayName}
        subtitle={
          <Text size="sm" c="dimmed">
            {/* Same helper as `MemberRow`'s own `label` above, so the
                credit on this card and the row in the member list above
                carry the same "(#a1b2c3)" for the same person -- which is
                the whole way to answer "who shared this?" when the IdP
                gives this app no showable name for anyone. */}
            Shared by {memberLabel(train.addedByName, train.addedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        }
        trailing={
          <Group gap="xs" wrap="nowrap">
            {/* Was a raw `train.status` string literal (e.g. "en_route")
                appended to the subtitle text -- the one place on this page
                that disagreed with every other status badge in the app.
                Same shared component `/` and `/track/mine` render off, so
                this card and those two pages can never drift on wording
                again (review §2.9). */}
            <TrackedTrainStatusBadge train={train} />
            {canRemove && (
              <RemoveGroupTrainButton groupId={groupId} trainSubscriptionId={train.trainSubscriptionId} />
            )}
          </Group>
        }
      />
    </Card>
  );
}

/** One journey shared into this group -- mirrors `SharedTrainRow` exactly,
 * one level up (a journey's identity/status instead of a single train's).
 * `canRemove` mirrors `groups::remove_journey_from_group`'s own
 * sharer-or-manager check exactly, the same way `SharedTrainRow`'s does.
 * Links out to `/journeys/{journeyId}` for full detail -- reachable by a
 * non-owning group member because of Task 4's `journey_readable_by`
 * widening, the one piece of this feature that isn't a pure
 * `SharedTrainRow` copy (that link resolving to a real page, rather than a
 * 404, for a member who isn't the journey's owner, IS the whole point of
 * this feature's one new authorization path). */
function SharedJourneyRow({
  groupId,
  journey,
  canManage,
  currentUserId,
}: {
  groupId: string;
  journey: GroupJourney;
  canManage: boolean;
  currentUserId: string | null;
}) {
  const canRemove = canManage || (currentUserId !== null && journey.addedBy === currentUserId);
  // Falls back to a plain leg-count label when no custom name was set --
  // mirrors trackedTrainDisplayName's own "compute a sensible default from
  // whatever's on the row" posture, kept inline here since it's a single
  // conditional rather than a reusable multi-field default-name
  // computation like that helper's.
  const displayName =
    journey.customName ?? (journey.legCount === 1 ? 'Untitled journey' : `Untitled journey (${journey.legCount} legs)`);
  return (
    <Card withBorder>
      <StatusRow
        title={
          <Link href={`/journeys/${journey.journeyId}`} style={{ textDecoration: 'none', color: 'inherit' }}>
            <Text fw={500}>{displayName}</Text>
          </Link>
        }
        subtitle={
          <Text size="sm" c="dimmed">
            Shared by {memberLabel(journey.addedByName, journey.addedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        }
        trailing={
          <Group gap="xs" wrap="nowrap">
            {/* `TrackedTrainStatusBadge`'s prop type requires a non-nullable
                `resolutionStatus`, but `GroupJourney.resolutionStatus` is
                nullable (a journey's first leg may have no bound train yet)
                -- coalesce to 'unresolved' here rather than widening that
                component's prop type, which `SharedTrainRow` above also
                relies on staying non-nullable. 'unresolved' already renders
                as the red "Unmatched" badge via that component's own
                `STATUS_LABELS` map, the accurate reading for a leg with no
                train bound yet. */}
            <TrackedTrainStatusBadge
              train={{
                resolutionStatus: journey.resolutionStatus ?? 'unresolved',
                status: journey.status,
                delayMinutes: journey.delayMinutes,
              }}
            />
            {canRemove && (
              <RemoveGroupJourneyButton groupId={groupId} journeyId={journey.journeyId} />
            )}
          </Group>
        }
      />
    </Card>
  );
}

/** One custom line shared into this group.
 *
 * Deliberately view-only for everyone except the line's owner, who reaches
 * their edit controls through `/lines/{id}` (this row links there) and not
 * from here. A grant conveys read access and nothing else: no member --
 * not even a group `admin`/`owner` -- can edit or delete a line they don't
 * own, and the backend refuses it regardless of what this page renders.
 *
 * `canRemove` mirrors `groups::remove_custom_line_grant`'s own
 * sharer-or-manager check exactly, the same way `SharedTrainRow`'s does:
 * "Stop sharing" only revokes the group's visibility and never touches the
 * line. */
function SharedCustomLineRow({
  groupId,
  line,
  report,
  canManage,
  currentUserId,
}: {
  groupId: string;
  line: GroupCustomLine;
  report: LineStatusReport | undefined;
  canManage: boolean;
  currentUserId: string | null;
}) {
  const canRemove = canManage || (currentUserId !== null && line.grantedBy === currentUserId);
  return (
    <Card withBorder>
      <StatusRow
        align="flex-start"
        title={
          <Link href={`/lines/${line.lineId}`} style={{ textDecoration: 'none', color: 'inherit' }}>
            <Text fw={500} lineClamp={2} style={{ minWidth: 0 }}>
              {line.lineName}
            </Text>
          </Link>
        }
        subtitle={
          <Text size="sm" c="dimmed">
            {/* `memberLabel`, not a hand-rolled fallback -- same helper
                the member rows and the shared-train row above use, so all
                three agree on blank-vs-null and on how an unnameable
                member is told apart (an opaque tag, never an email). */}
            Shared by {memberLabel(line.grantedByName, line.grantedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        }
        trailing={
          <Group gap="xs" wrap="nowrap">
            {report && <StatusBadge severity={worstStatus(report).statusSeverity} />}
            {canRemove && (
              <RemoveCustomLineGrantButton
                groupId={groupId}
                lineId={line.lineId}
                lineName={line.lineName}
              />
            )}
          </Group>
        }
      />
    </Card>
  );
}
