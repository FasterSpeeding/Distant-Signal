import { Card, Group, Stack } from '@mantine/core';
import {
  PageSkeleton,
  SkeletonCardGrid,
  SkeletonPill,
  SkeletonRows,
  SkeletonSection,
  SkeletonText,
} from '@/components/PageSkeleton';
import { tokens } from '@/lib/theme';

/** The home page's skeleton, shown while the page fetches statuses, the
 * visitor's pins and their journeys. It lives in the `(home)` route group,
 * not at `app/`: a root `app/loading.tsx` would be the nearest loading
 * boundary for every route without one of its own, so /status or /account
 * would flash the home page's shape while they load.
 *
 * The session isn't known yet, so this draws the logged-in layout, the one
 * people come back to: the "Current journey" card (`CurrentJourneyCard`'s
 * padding, name, preview line and two facts, inside the same
 * `.currentJourneyCard` border), an "Upcoming journeys" list, then the
 * "Your lines" grid and a list section. A visitor without a journey under
 * way, or logged out, sees those sections give way to the next. */
export default function HomeLoading() {
  return (
    <PageSkeleton label="Loading the home page" gap="xl">
      <SkeletonSection>
        <Card {...tokens.card.static} padding="lg" className="currentJourneyCard">
          <Stack gap="sm">
            <Stack gap={4}>
              <Group justify="space-between" wrap="nowrap" gap="xs">
                <SkeletonText width="45%" />
                <SkeletonPill width={88} />
              </Group>
              <SkeletonText size="sm" width="40%" />
            </Stack>
            <SkeletonText lines={2} size="sm" width="70%" />
          </Stack>
        </Card>
      </SkeletonSection>
      <SkeletonSection>
        <SkeletonRows count={2} />
      </SkeletonSection>
      <SkeletonSection>
        <SkeletonCardGrid count={3} />
      </SkeletonSection>
      <SkeletonSection>
        <SkeletonRows count={2} />
      </SkeletonSection>
    </PageSkeleton>
  );
}
