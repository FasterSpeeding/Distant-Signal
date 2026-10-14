import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import {
  PageSkeleton,
  SkeletonCardGrid,
  SkeletonPageHeader,
  SkeletonRows,
  SkeletonStops,
  SkeletonText,
} from './PageSkeleton';
import { TrainPageSkeleton } from './TrainPageSkeleton';
import HomeLoading from '@/app/(home)/loading';
import LineLoading from '@/app/lines/[id]/loading';
import TrainLoading from '@/app/train/[uid]/[date]/loading';
import TrackedTrainLoading from '@/app/train/by-id/[trackingId]/loading';
import TrackLoading from '@/app/track/mine/loading';

/** Text a reader gets: Mantine's `SimpleGrid` injects a `<style>` tag,
 * whose CSS is text content but never read out. */
function spokenText(root: HTMLElement): string {
  const copy = root.cloneNode(true) as HTMLElement;
  copy.querySelectorAll('style').forEach((style) => style.remove());
  return copy.textContent;
}

function skeletons(root: HTMLElement) {
  return root.querySelectorAll('.mantine-Skeleton-root');
}

describe('PageSkeleton', () => {
  it('is one busy status region named by its label, with everything drawn hidden', () => {
    renderWithMantine(
      <PageSkeleton label="Loading line">
        <SkeletonText lines={3} />
      </PageSkeleton>,
    );
    const status = screen.getByRole('status');
    expect(status).toHaveAttribute('aria-busy', 'true');
    expect(status).toHaveTextContent(/^Loading line$/);
    expect(skeletons(status)).toHaveLength(3);
    for (const skeleton of skeletons(status)) {
      expect(skeleton.closest('[aria-hidden="true"]')).not.toBeNull();
    }
  });

  it('draws a shorter last line for a paragraph', () => {
    const { container } = renderWithMantine(<SkeletonText lines={3} />);
    const lines = skeletons(container);
    expect(lines).toHaveLength(3);
    expect(lines.item(2).getAttribute('style')).toContain('60%');
  });

  it('draws the header controls and subline only when asked', () => {
    const { container: bare } = renderWithMantine(<SkeletonPageHeader />);
    expect(skeletons(bare)).toHaveLength(1);
    const { container: full } = renderWithMantine(<SkeletonPageHeader actions={2} subline />);
    expect(skeletons(full)).toHaveLength(4);
  });

  it('draws real bordered cards for grids and rows, so card edges do not move', () => {
    const { container: grid } = renderWithMantine(<SkeletonCardGrid count={3} />);
    expect(grid.querySelectorAll('.mantine-Card-root')).toHaveLength(3);
    const { container: rows } = renderWithMantine(<SkeletonRows count={2} />);
    expect(rows.querySelectorAll('.mantine-Card-root')).toHaveLength(2);
  });

  it('draws one row per stop', () => {
    const { container } = renderWithMantine(<SkeletonStops count={5} />);
    expect(skeletons(container)).toHaveLength(10);
  });
});

describe('route loading skeletons', () => {
  it.each([
    ['home', HomeLoading, 'Loading the home page'],
    ['line', LineLoading, 'Loading line'],
    ['train', TrainLoading, 'Loading train'],
    ['tracked train', TrackedTrainLoading, 'Loading train'],
    ['track', TrackLoading, 'Loading your trains'],
  ])('the %s page announces one label and no ellipsis', (_name, Loading, label) => {
    renderWithMantine(<Loading />);
    // `getByRole` throws unless there is exactly one status region.
    const status = screen.getByRole('status');
    expect(spokenText(status)).toBe(label);
    expect(skeletons(status).length).toBeGreaterThan(3);
    // No real heading yet: the page's h1 arrives with its data.
    expect(screen.queryByRole('heading')).toBeNull();
  });

  it('the two train routes share one skeleton', () => {
    const { container: a } = renderWithMantine(<TrainLoading />);
    const { container: b } = renderWithMantine(<TrainPageSkeleton />);
    expect(a.innerHTML).toBe(b.innerHTML);
  });
});
