import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import type { PredictedStart } from '@/lib/types';
import { PredictedStartNotice, predictedStartHeadline } from './PredictedStartNotice';

function prediction(overrides: Partial<PredictedStart> = {}): PredictedStart {
  return {
    lateMinutes: 7,
    confidence: 'high',
    basis: 'previous-working',
    computedAt: '2026-10-22T09:00:00Z',
    previousWorking: {
      trainUid: 'P12345',
      serviceDate: '2026-10-22',
      link: 'inferred',
      status: 'en_route',
      delayMinutes: 12,
      observedAt: '2026-10-22T08:58:00Z',
      bookedTurnaroundMinutes: 9,
    },
    ...overrides,
  };
}

describe('PredictedStartNotice', () => {
  it('renders nothing without a prediction', () => {
    renderWithMantine(<PredictedStartNotice prediction={null} />);
    expect(screen.queryByText(/start late/)).not.toBeInTheDocument();
  });

  it('gives the minutes at high or medium confidence, with why, as an estimate', () => {
    renderWithMantine(<PredictedStartNotice prediction={prediction()} />);
    expect(screen.getByText('Expected to start about 7 min late')).toBeInTheDocument();
    expect(screen.getByText('Estimate')).toBeInTheDocument();
    const link = screen.getByRole('link', { name: 'incoming train' });
    expect(link).toHaveAttribute('href', '/train/P12345/2026-10-22');
    expect(link.closest('p')).toHaveTextContent('Based on the incoming train, running 12 min late.');
  });

  it('says only "may start late" at low confidence, never a number', () => {
    renderWithMantine(
      <PredictedStartNotice
        prediction={prediction({
          confidence: 'low',
          lateMinutes: null,
          previousWorking: { ...prediction().previousWorking, delayMinutes: 25 },
        })}
      />,
    );
    expect(screen.getByText('May start late')).toBeInTheDocument();
    expect(screen.queryByText(/about \d+ min/)).not.toBeInTheDocument();
    expect(screen.getByText(/running 25 min late/)).toBeInTheDocument();
  });

  it('explains a prediction two links back', () => {
    renderWithMantine(
      <PredictedStartNotice
        prediction={prediction({
          confidence: 'low',
          lateMinutes: null,
          previousWorking: { ...prediction().previousWorking, status: 'awaiting_activation', delayMinutes: null },
        })}
      />,
    );
    expect(screen.getByText(/which is also expected to start late/)).toBeInTheDocument();
  });

  it('headline: a medium prediction keeps its number', () => {
    expect(predictedStartHeadline(prediction({ confidence: 'medium', lateMinutes: 3 }))).toBe(
      'Expected to start about 3 min late',
    );
  });
});
