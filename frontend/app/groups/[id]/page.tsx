import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import {
  getGroup,
  getGroupMembers,
  getGroupTrains,
  getSession,
  ApiNotFoundError,
  ApiUnauthorizedError,
} from '@/lib/api';
import { RemoveMemberButton } from '@/components/RemoveMemberButton';
import { PromoteMemberButton } from '@/components/PromoteMemberButton';
import { LeaveGroupButton } from '@/components/LeaveGroupButton';
import { RenameGroupButton } from '@/components/RenameGroupButton';
import { DeleteGroupButton } from '@/components/DeleteGroupButton';
import { GroupInviteLinkCard } from '@/components/GroupInviteLinkCard';
import { RemoveGroupTrainButton } from '@/components/RemoveGroupTrainButton';
import { AddTrainToGroupButton } from '@/components/AddTrainToGroupButton';
import { LoginLink } from '@/components/LoginLink';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { GroupMember, GroupTrain } from '@/lib/types';

export const revalidate = 0;

/** `/groups/{id}` -- group detail: the group's own header controls
 * (rename/delete/leave), members (with role badges, remove/promote/
 * invite-link), and the trains shared into the group.
 *
 * Every control here is gated on the caller's role using the SAME
 * predicate the corresponding backend handler uses
 * (`crates/api/src/routes/groups.rs`), not a coarser one -- `canManage`
 * (`admin`/`owner`) for rename, member removal, and the invite-link card;
 * `viewerIsOwner` for promotion and group deletion; sharer-or-manager for
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

  const [members, trains, session] = await Promise.all([
    getGroupMembers(id),
    getGroupTrains(id),
    getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null })),
  ]);
  const currentUserId = session.authenticated ? session.id : null;
  const canManage = group.role === 'owner' || group.role === 'admin';
  // Distinct from `canManage`: the backend gates promotion and group
  // deletion on `GroupRole::is_owner`, NOT `can_manage`
  // (`crates/api/src/routes/groups.rs`), so showing either control to an
  // admin would be offering a button whose only outcome is a 403.
  const viewerIsOwner = group.role === 'owner';

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>{group.name}</Title>
        <Group gap="xs">
          {canManage && <RenameGroupButton groupId={id} currentName={group.name} />}
          {viewerIsOwner && <DeleteGroupButton groupId={id} name={group.name} />}
          {currentUserId && <LeaveGroupButton groupId={id} currentUserId={currentUserId} />}
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
        {canManage && <GroupInviteLinkCard groupId={id} inviteLink={group.inviteLink} />}
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
  const label = member.displayName ?? 'A member';
  const isOwner = member.role === 'owner';
  return (
    <Group justify="space-between" wrap="nowrap">
      <Group gap="xs">
        <Text>{label}</Text>
        <Badge variant="outline">{member.role}</Badge>
      </Group>
      <Group gap="xs">
        {/* `viewerIsOwner`, not `canManage`: `promote_member` is gated on
            `GroupRole::is_owner` server-side, so an admin who clicked this
            would only ever get a 403. */}
        {viewerIsOwner && member.role === 'member' && <PromoteMemberButton groupId={groupId} userId={member.userId} />}
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
      <Group justify="space-between" wrap="nowrap">
        <Stack gap={4}>
          <Text fw={500}>{displayName}</Text>
          <Text size="sm" c="dimmed">
            Shared by {train.addedByName ?? 'a member'}
            {train.status && ` · ${train.status}`}
            {train.delayMinutes !== null && train.delayMinutes > 0 && ` · ${train.delayMinutes}m late`}
          </Text>
        </Stack>
        {canRemove && (
          <RemoveGroupTrainButton groupId={groupId} trainSubscriptionId={train.trainSubscriptionId} />
        )}
      </Group>
    </Card>
  );
}
