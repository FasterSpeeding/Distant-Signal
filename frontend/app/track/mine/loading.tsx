import { PageSkeleton, SkeletonPageHeader, SkeletonRows, SkeletonSection } from '@/components/PageSkeleton';

/** The Track page's skeleton (`/track/mine`): the heading with its
 * actions, then the journeys section and the list of tracked trains, as
 * rows. The heading is a bar too, so it doesn't depend on the page's
 * wording. */
export default function TrackLoading() {
  return (
    <PageSkeleton label="Loading your trains" gap="lg">
      <SkeletonPageHeader width="30%" />
      <SkeletonSection gap="xs">
        <SkeletonRows count={2} />
      </SkeletonSection>
      <SkeletonRows count={3} />
    </PageSkeleton>
  );
}
