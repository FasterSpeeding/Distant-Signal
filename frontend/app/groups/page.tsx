import { Badge, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { getMyGroups } from '@/lib/api';
import { AutoOpenLoginPrompt } from './AutoOpenLoginPrompt';
import { TextLink } from '@/components/TextLink';
import type { GroupSummary } from '@/lib/types';

// See app/page.tsx's own `revalidate = 0` comment: no dynamic segment on
// this route, so without this Next.js tries to prerender it during `next
// build`, which fails since the `api` service only exists at runtime.
export const revalidate = 0;

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead). This route
 * takes no params of any kind, so a static export is the only shape that
 * makes sense here.
 *
 * Title matches the page's own `<h1>` ("Groups"), which is also this
 * route's nav label (`GroupsNavItem` in `app/layout.tsx`).
 *
 * The description is written for the reader who will actually see it.
 * Every consumer of this metadata is a link-unfurler bot, and none carry a
 * session cookie, so `getMyGroups()` 401s for them and the ONLY branch
 * they can render is the "Log in to see your groups." one -- the same
 * reasoning `app/page.tsx` spells out for its own anonymous-branch copy.
 * So this explains what a group IS and hedges the listing behind logging
 * in, rather than describing a list the recipient of the link will not
 * find on the page they land on.
 *
 * "tracked trains and custom lines" is both halves deliberately: a group
 * carries both (`app/groups/[id]/page.tsx` renders a shared-trains section
 * AND a "Shared custom lines" one, via `getGroupTrains`/
 * `getGroupCustomLines`), even though this list page's own empty-state
 * sentence happens to mention only trains. "your role in it" names the
 * second `Badge` on each row (`group.role`), alongside the member count
 * the first one carries. */
const METADATA_TITLE = 'Groups — Distant Signal';
const METADATA_DESCRIPTION =
  'Groups are how tracked trains and custom lines get shared with other people. Log in to see the ones you belong to — each with its member count and your role in it — or create a group and invite people to it.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

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
