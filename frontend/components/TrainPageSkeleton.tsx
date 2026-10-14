import { PageSkeleton, SkeletonPageHeader, SkeletonStops, SkeletonText } from './PageSkeleton';

/** The train page skeleton, shared by `/train/[uid]/[date]` and
 * `/train/by-id/[trackingId]`, which lay out the same way: the train's
 * name ("08:42 Woking to London Waterloo") with its identifiers under it
 * and the track and share controls beside it, then the status line and
 * the calling points. */
export function TrainPageSkeleton() {
  return (
    <PageSkeleton label="Loading train">
      <SkeletonPageHeader actions={2} subline width="70%" />
      <SkeletonText width="40%" />
      <SkeletonStops count={8} />
    </PageSkeleton>
  );
}
