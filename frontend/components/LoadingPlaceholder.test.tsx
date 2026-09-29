import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Skeleton } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { LoadingPlaceholder } from './LoadingPlaceholder';

describe('LoadingPlaceholder', () => {
  it('is a busy status region whose visible label is its announced content', () => {
    renderWithMantine(<LoadingPlaceholder label="Loading trends…" height={320} />);
    const status = screen.getByRole('status');
    expect(status).toHaveAttribute('aria-busy', 'true');
    expect(status).toHaveTextContent('Loading trends…');
    expect(screen.getByText('Loading trends…')).toBeVisible();
  });

  it('draws one aria-hidden skeleton at the given height', () => {
    renderWithMantine(<LoadingPlaceholder label="Loading trends…" height={320} />);
    const skeletons = screen.getByRole('status').querySelectorAll('.mantine-Skeleton-root');
    expect(skeletons).toHaveLength(1);
    expect(skeletons[0]).toHaveAttribute('aria-hidden', 'true');
  });

  it('renders custom skeleton children inside an aria-hidden wrapper', () => {
    renderWithMantine(
      <LoadingPlaceholder label="Looking up disruptions…">
        <Skeleton height={20} />
        <Skeleton height={60} />
      </LoadingPlaceholder>,
    );
    const skeletons = screen.getByRole('status').querySelectorAll('.mantine-Skeleton-root');
    expect(skeletons).toHaveLength(2);
    for (const skeleton of skeletons) {
      expect(skeleton.closest('[aria-hidden="true"]')).not.toBeNull();
    }
  });

  it('renders just the label when given neither a height nor children', () => {
    renderWithMantine(<LoadingPlaceholder label="Loading history…" />);
    expect(screen.getByRole('status').querySelector('.mantine-Skeleton-root')).toBeNull();
    expect(screen.getByRole('status')).toHaveTextContent('Loading history…');
  });
});
