import { Group, Stack, Text } from '@mantine/core';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';
import { StateBadge } from '@/components/StateBadge';
import { StatusBadge } from '@/components/StatusBadge';
import { StatusRow } from '@/components/StatusRow';
import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from '@/lib/memberLabel';
import type { MergedSharedCustomLine } from '@/lib/sharedCustomLines';
import { worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

/* The home page's "Lines shared with you" row. */

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
    <ClickableCard>
      <Stack gap={4}>
        <StatusRow
          title={
            <Text fw={500} lineClamp={2} miw={0}>
              <CardTitleLink href={`/lines/${line.lineId}`}>{line.lineName}</CardTitleLink>
            </Text>
          }
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
                email. Same `memberLabel` helper `/journeys`' shared-train
                row and `/groups/{id}` use, so none of them can drift. */}
          <Text size="sm" c="dimmed">
            Shared by {memberLabel(line.grantedByName, line.grantedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        </Group>
      </Stack>
    </ClickableCard>
  );
}
