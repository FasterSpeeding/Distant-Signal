import {
  PageSkeleton,
  SkeletonPageHeader,
  SkeletonRows,
  SkeletonSection,
  SkeletonText,
} from '@/components/PageSkeleton';

/** A line page's skeleton: the line name with its badge and share button,
 * the status reason, the history link, then the "Trains on this line"
 * section. The page's own Suspense fallbacks take over for trains and
 * trends once the header has rendered. */
export default function LineLoading() {
  return (
    <PageSkeleton label="Loading line">
      <SkeletonPageHeader actions={2} width="50%" />
      <SkeletonText lines={2} />
      <SkeletonText size="sm" width="30%" />
      <SkeletonSection>
        <SkeletonRows count={3} />
      </SkeletonSection>
    </PageSkeleton>
  );
}
