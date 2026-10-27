import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { CancellationNotice } from './CancellationNotice';
import type { Cancellation } from '@/lib/types';

const RAYNES = { crs: 'RAY', name: 'Raynes Park', stopIndex: 5 };
const WATERLOO = { crs: 'WAT', name: 'London Waterloo', stopIndex: 11 };

function base(overrides: Partial<Cancellation>): Cancellation {
  return {
    kind: 'full',
    startsFrom: null,
    terminatesAt: null,
    from: null,
    to: null,
    sections: [],
    reason: null,
    at: null,
    ...overrides,
  };
}

describe('CancellationNotice', () => {
  it('reads a full cancellation with a board reason as one sentence', () => {
    renderWithMantine(
      <CancellationNotice
        cancellation={base({
          reason: {
            code: null,
            text: 'This train has been cancelled because of a fault with the signalling system',
            source: 'ldbws',
          },
        })}
      />,
    );
    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('Cancelled');
    expect(alert).toHaveTextContent('Reason: a fault with the signalling system.');
    expect(alert).toHaveTextContent('Reason from National Rail Enquiries');
    expect(alert).toHaveAttribute('data-cancellation-kind', 'full');
  });

  it('says a terminated-short train ends early, the section it misses and why', () => {
    renderWithMantine(
      <CancellationNotice
        cancellation={base({
          kind: 'partial',
          terminatesAt: RAYNES,
          from: RAYNES,
          to: WATERLOO,
          sections: [{ from: RAYNES, to: WATERLOO }],
          reason: { code: 'JP', text: 'Failure to maintain vegetation', source: 'trust' },
        })}
      />,
    );
    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('Terminates at Raynes Park');
    expect(alert).toHaveTextContent('Cancelled between Raynes Park and London Waterloo.');
    expect(alert).toHaveTextContent('Reason: failure to maintain vegetation.');
    expect(alert).toHaveTextContent('Reason recorded by Network Rail');
  });

  it('says plainly when no reason has been given', () => {
    renderWithMantine(<CancellationNotice cancellation={base({})} />);
    expect(screen.getByText('This service was cancelled. No reason has been given.')).toBeInTheDocument();
  });

  it('does not repeat a middle section already in the headline', () => {
    renderWithMantine(
      <CancellationNotice cancellation={base({ kind: 'partial', sections: [{ from: RAYNES, to: WATERLOO }] })} />,
    );
    expect(screen.getAllByText(/Cancelled between Raynes Park and London Waterloo/)).toHaveLength(1);
    expect(screen.getByText('No reason has been given.')).toBeInTheDocument();
  });
});
