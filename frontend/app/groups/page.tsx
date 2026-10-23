import { Group, Stack, Text, Title } from '@mantine/core';
import { groupRoleLabel } from '@/lib/memberLabel';
import type { Metadata } from 'next';
import { getMyGroups } from '@/lib/api';
import { AutoOpenLoginPrompt } from './AutoOpenLoginPrompt';
import { LoginControl } from '@/components/LoginControl';
import { TextLink } from '@/components/TextLink';
import type { GroupSummary } from '@/lib/types';
import { pageMetadata } from '@/lib/pageMetadata';
import { ButtonLink } from '@/components/ButtonLink';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';

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
 * route's nav label (`GROUPS_DESTINATION` in `lib/navLinks.ts`, rendered
 * in the account menu at `md` and up and in the nav drawer below it).
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
const METADATA_TITLE = 'Groups';
const METADATA_DESCRIPTION =
  'Share tracked trains and custom lines with other people. Log in to see your groups or create one.';

export const metadata: Metadata = pageMetadata(METADATA_TITLE, METADATA_DESCRIPTION);

/** `/groups` -- list of the current user's groups: name, member count,
 * role badge, "Create group" CTA (spec §6).
 *
 * `maw={640}` on both returns below (review §3.2.8, deliberately on top of
 * Task 1.1's root-cause `<main>` fix, not instead of it -- see that task's
 * own note): a list of short name/badge rows stretching the full content
 * width reads as sparse rather than as a deliberate layout, the same
 * "measure" concern typography guidance raises for any list of short
 * lines. */
export default async function GroupsPage() {
  const groups = await getMyGroups();

  if (groups === null) {
    return (
      <Stack p="lg" gap="md" maw={640}>
        <Title order={1}>Groups</Title>
        {/* Server-rendered, same as the two already-correct routes
            (app/train/by-id/[trackingId]/page.tsx,
            app/groups/[id]/page.tsx): a link-unfurler bot or a
            pre-hydration visitor sees this sentence even though it can
            never run the client-only AutoOpenLoginPrompt modal below,
            which stays as progressive enhancement on top of it.

            A filled `LoginControl`, not the underlined `LoginControl` text this
            used to be (review §2.16) -- the only action an anonymous
            visitor can take on this route had markedly less visual weight
            than the "Create group" `TextLink` an authenticated visitor sees
            in its place. */}
        <LoginControl variant="button" title="Log in (needs a Distant Signal account)">
          Log in to see your groups
        </LoginControl>
        <AutoOpenLoginPrompt>Log in to see your groups.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="lg" maw={640}>
      <Group justify="space-between" align="baseline">
        <Title order={1}>Groups</Title>
        {/* Review §2.7/M20: the empty state below has its own, single
            "Create a group" call to action in the body -- a second,
            differently-styled link up here would duplicate it 40px away and
            (with nothing else in this row to balance `justify="space-between"`)
            float alone in the middle of the header on a wide viewport. This
            link earns its place once there are rows to scroll past. */}
        {groups.length > 0 && <TextLink href="/groups/new">Create group</TextLink>}
      </Group>
      {groups.length === 0 ? (
        <Stack gap="sm" align="flex-start">
          {/* Review §2.7/M20: body colour, not `c="dimmed"` -- this sentence
              is the only guidance on the page, not metadata to be
              de-emphasised. "journeys and custom lines" matches this route's
              own `<meta description>` above (a group shares both); the old
              copy said "tracked trains" alone. The second clause names the
              far more common way most people actually end up in a group --
              being invited -- which the page previously never mentioned at
              all. */}
          <Text>
            You&apos;re not in any groups yet. Create one to share journeys and custom lines with other people, or ask a
            group member to send you their invite link if you&apos;re joining an existing one.
          </Text>
          <ButtonLink href="/groups/new">Create a group</ButtonLink>
        </Stack>
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
  // `ClickableCard` (review §3.2.5): the name is the link, stretched over
  // the card, with the clickable card's hover, press and focus states.
  // The member count and role are dimmed text, not badges (round 2):
  // neither is a state.
  return (
    <ClickableCard>
      <Group justify="space-between" wrap="nowrap">
        <Text fw={500}>
          <CardTitleLink href={`/groups/${group.id}`}>{group.name}</CardTitleLink>
        </Text>
        <Group gap={6} wrap="nowrap">
          <Text size="sm" c="dimmed">
            {group.memberCount} member{group.memberCount === 1 ? '' : 's'}
          </Text>
          <Text size="sm" c="dimmed" aria-hidden>
            ·
          </Text>
          <Text size="sm" c="dimmed">
            {groupRoleLabel(group.role)}
          </Text>
        </Group>
      </Group>
    </ClickableCard>
  );
}
