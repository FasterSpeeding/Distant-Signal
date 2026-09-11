import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import { getGroup, getGroupMembers, getGroupTrains, getSession, ApiNotFoundError } from '@/lib/api';
import { RemoveMemberButton } from '@/components/RemoveMemberButton';
import { PromoteMemberButton } from '@/components/PromoteMemberButton';
import { LeaveGroupButton } from '@/components/LeaveGroupButton';
import { GroupInviteLinkCard } from '@/components/GroupInviteLinkCard';
import { RemoveGroupTrainButton } from '@/components/RemoveGroupTrainButton';
import { AddTrainToGroupButton } from '@/components/AddTrainToGroupButton';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { GroupMember, GroupTrain } from '@/lib/types';

export const revalidate = 0;

/** `/groups/{id}` -- group detail: members (with role badges, remove/
 * promote/leave/invite-link) and the trains shared into the group. The
 * "Add one of my trains" picker is added on top of this page in Task 13;
 * this task ships the full read + remove/promote/leave surface on its own. */
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
    throw err;
  }

  const [members, trains, session] = await Promise.all([
    getGroupMembers(id),
    getGroupTrains(id),
    getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null })),
  ]);
  const currentUserId = session.authenticated ? session.id : null;
  const canManage = group.role === 'owner' || group.role === 'admin';

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>{group.name}</Title>
        {currentUserId && <LeaveGroupButton groupId={id} currentUserId={currentUserId} />}
      </Group>

      <Stack gap="sm">
        <Title order={2}>Members</Title>
        {members.map((member) => (
          <MemberRow key={member.userId} groupId={id} member={member} canManage={canManage} />
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
          trains.map((train) => <SharedTrainRow key={train.trainSubscriptionId} groupId={id} train={train} />)
        )}
      </Stack>
    </Stack>
  );
}

function MemberRow({ groupId, member, canManage }: { groupId: string; member: GroupMember; canManage: boolean }) {
  const label = member.name ?? member.email ?? 'A member';
  const isOwner = member.role === 'owner';
  return (
    <Group justify="space-between" wrap="nowrap">
      <Group gap="xs">
        <Text>{label}</Text>
        <Badge variant="outline">{member.role}</Badge>
      </Group>
      <Group gap="xs">
        {canManage && member.role === 'member' && <PromoteMemberButton groupId={groupId} userId={member.userId} />}
        {canManage && !isOwner && <RemoveMemberButton groupId={groupId} userId={member.userId} name={label} />}
      </Group>
    </Group>
  );
}

function SharedTrainRow({ groupId, train }: { groupId: string; train: GroupTrain }) {
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
        <RemoveGroupTrainButton groupId={groupId} trainSubscriptionId={train.trainSubscriptionId} />
      </Group>
    </Card>
  );
}
