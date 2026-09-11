import { Badge, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyGroups } from '@/lib/api';
import { AutoOpenLoginPrompt } from './AutoOpenLoginPrompt';
import { TextLink } from '@/components/TextLink';
import type { GroupSummary } from '@/lib/types';

// See app/page.tsx's own `revalidate = 0` comment: no dynamic segment on
// this route, so without this Next.js tries to prerender it during `next
// build`, which fails since the `api` service only exists at runtime.
export const revalidate = 0;

/** `/groups` -- list of the current user's groups: name, member count,
 * role badge, "Create group" CTA (spec §6). */
export default async function GroupsPage() {
  const groups = await getMyGroups();

  if (groups === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Groups</Title>
        <AutoOpenLoginPrompt>Log in to see your groups.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>Groups</Title>
        <TextLink href="/groups/new">Create group</TextLink>
      </Group>
      {groups.length === 0 ? (
        <Text c="dimmed">
          You&apos;re not in any groups yet. <Link href="/groups/new">Create one</Link> to share tracked trains
          with other people.
        </Text>
      ) : (
        <Stack gap="xs">
          {groups.map((group) => (
            <GroupRow key={group.id} group={group} />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function GroupRow({ group }: { group: GroupSummary }) {
  // Plain <Link> wrapping the Card, not `component={Link}` on the Mantine
  // polymorphic prop -- this is a Server Component, and passing `Link` as
  // a value into a Mantine `component` prop from one previously broke
  // `next build`'s Server/Client boundary check (see `app/layout.tsx`'s
  // own comment on its nav-bar `<Link>` for the same reasoning).
  return (
    <Link href={`/groups/${group.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder>
        <Group justify="space-between">
          <Text fw={500}>{group.name}</Text>
          <Group gap="xs">
            <Badge variant="light">
              {group.memberCount} member{group.memberCount === 1 ? '' : 's'}
            </Badge>
            <Badge variant="outline">{group.role}</Badge>
          </Group>
        </Group>
      </Card>
    </Link>
  );
}
