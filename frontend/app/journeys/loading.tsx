import { PageSkeleton, SkeletonPageHeader, SkeletonRows, SkeletonSection } from '@/components/PageSkeleton';

/** The Journeys page's skeleton (`/journeys`): the heading with its
 * actions, then the upcoming and past lists, as rows. The heading is a bar
 * too, so it doesn't depend on the page's wording. */
export default function JourneysLoading() {
  return (
    <PageSkeleton label="Loading your journeys" gap="xl">
      <SkeletonPageHeader width="30%" />
      <SkeletonSection gap="xs">
        <SkeletonRows count={2} />
      </SkeletonSection>
      <SkeletonSection gap="xs">
        <SkeletonRows count={3} />
      </SkeletonSection>
    </PageSkeleton>
  );
}
