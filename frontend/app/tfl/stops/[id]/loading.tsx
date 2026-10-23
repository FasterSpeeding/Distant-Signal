import {
  PageSkeleton,
  SkeletonCardGrid,
  SkeletonHeading,
  SkeletonPill,
  SkeletonRows,
  SkeletonSection,
} from '@/components/PageSkeleton';

/** A TfL stop page's skeleton while TfL's arrivals load: the stop name and
 * its mode badges, the journey button, the arrivals, then the line status
 * cards. */
export default function TflStopLoading() {
  return (
    <PageSkeleton label="Loading stop" gap="lg">
      <SkeletonHeading width="40%" />
      <SkeletonPill width={88} />
      <SkeletonPill kind="button" width={170} />
      <SkeletonSection>
        <SkeletonRows count={4} />
      </SkeletonSection>
      <SkeletonSection>
        <SkeletonCardGrid count={2} />
      </SkeletonSection>
    </PageSkeleton>
  );
}
