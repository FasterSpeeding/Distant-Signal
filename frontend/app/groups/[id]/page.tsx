import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import {
  getGroup,
  getGroupCustomLines,
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
import { StatusBadge } from '@/components/StatusBadge';
import { StatusRow } from '@/components/StatusRow';
import { LoginLink } from '@/components/LoginLink';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import { worstStatus } from '@/lib/severity';
import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from '@/lib/memberLabel';
import { getSiteOrigin } from '@/lib/siteOrigin';
import type { GroupCustomLine, GroupMember, GroupTrain, LineStatusReport } from '@/lib/types';

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

  const [members, trains, customLines, session] = await Promise.all([
    getGroupMembers(id),
    getGroupTrains(id),
    getGroupCustomLines(id),
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
  // Only actually used by GroupInviteLinkCard below (canManage-gated), but
  // resolved unconditionally rather than behind an `if (canManage)` --
  // it's a cheap header/env read, and keeping it unconditional means this
  // call site can't silently start passing a stale/undefined origin if a
  // future edit reorders things around the `canManage` check.
  const origin = await getSiteOrigin();

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>{group.name}</Title>
        <Group gap="xs">
          {canManage && <RenameGroupButton groupId={id} currentName={group.name} />}
          {viewerIsOwner && <DeleteGroupButton groupId={id} name={group.name} />}
          {currentUserId && (
            <LeaveGroupButton
              groupId={id}
              currentUserId={currentUserId}
              willDeleteGroup={viewerIsOwner && group.memberCount === 1}
            />
          )}
        </Group>
      </Group>

      <Stack gap="sm">
        <Title order={2}>Members</Title>
        {members.map((member) => (
          <MemberRow
            key={member.userId}
            groupId={id}
            member={member}
            canManage={canManage}
            viewerIsOwner={viewerIsOwner}
          />
        ))}
        {canManage && <GroupInviteLinkCard groupId={id} inviteLink={group.inviteLink} origin={origin} />}
      </Stack>

      <Divider />

      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={2}>Shared trains</Title>
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
          <Title order={2}>Shared custom lines</Title>
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
    </Stack>
  );
}

function MemberRow({
  groupId,
  member,
  canManage,
  viewerIsOwner,
}: {
  groupId: string;
  member: GroupMember;
  canManage: boolean;
  viewerIsOwner: boolean;
}) {
  // `memberLabel`, not a bare `displayName`: a member whose identity
  // provider has no name on file for them (or only an email-shaped one,
  // which the backend declines to show) renders as the generic
  // placeholder, suffixed with their `displayTag` so several such members
  // are still separate rows rather than one repeated "A member". See
  // `lib/memberLabel.ts`.
  const label = memberLabel(member.displayName, member.displayTag);
  const isOwner = member.role === 'owner';
  return (
    <Group justify="space-between" wrap="nowrap">
      <Group gap="xs">
        <Text>{label}</Text>
        <Badge variant="outline">{member.role}</Badge>
      </Group>
      <Group gap="xs">
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
    </Group>
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
            {train.status && ` · ${train.status}`}
            {train.delayMinutes !== null && train.delayMinutes > 0 && ` · ${train.delayMinutes}m late`}
          </Text>
        }
        trailing={
          canRemove && (
            <RemoveGroupTrainButton groupId={groupId} trainSubscriptionId={train.trainSubscriptionId} />
          )
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
