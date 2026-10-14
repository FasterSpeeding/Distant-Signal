import {
  PageSkeleton,
  SkeletonCardGrid,
  SkeletonHeading,
  SkeletonRows,
  SkeletonSection,
  SkeletonText,
} from '@/components/PageSkeleton';

/** The home page's skeleton, shown while the page fetches statuses and the
 * visitor's pins. It lives in the `(home)` route group, not at `app/`: a
 * root `app/loading.tsx` would be the nearest loading boundary for every
 * route without one of its own, so /status or /account would flash the
 * home page's shape while they load.
 *
 * The page has two layouts (logged out: heading, tagline, "Right now";
 * logged in: "Your lines" grid, then stations and operators), and the
 * session isn't known yet. This draws their common shape: an h1 with a
 * line under it, a grid of status cards, then a list section. */
export default function HomeLoading() {
  return (
    <PageSkeleton label="Loading the home page" gap="xl">
      <SkeletonHeading width="40%" />
      <SkeletonText lines={2} />
      <SkeletonCardGrid count={3} />
      <SkeletonSection>
        <SkeletonRows count={2} />
      </SkeletonSection>
    </PageSkeleton>
  );
}
