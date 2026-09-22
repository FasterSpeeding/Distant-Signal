import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { OperatorStatusCard } from './OperatorStatusCard';
import type { OperatorSummary } from '@/lib/types';

// PinToggle calls useRouter() from next/navigation, which throws
// "invariant expected app router to be mounted" when rendered outside a
// real Next.js App Router tree (as in these unit tests). Stub it so the
// component can render; router.refresh() itself isn't under test here.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/operators',
  useSearchParams: () => new URLSearchParams(''),
}));

const operator: OperatorSummary = {
  code: 'VT',
  name: 'Virgin Trains',
  lineIds: ['wcml', 'ecml'],
  worstSeverity: 9,
  reason: 'Signal failure',
  sampleStats: { total: 20, delayed: 8, cancelled: 2, skipped: 0, avgDelayMinutes: 7.25 },
  computedAt: '2026-07-15T09:00:00Z',
};

describe('OperatorStatusCard', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders the operator name', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    expect(screen.getByText('Virgin Trains')).toBeInTheDocument();
  });

  it('renders the worst status badge', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    expect(screen.getByText('Minor Delays')).toBeInTheDocument();
  });

  it('renders the reason', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
  });

  it('renders the default reason text when no reason is provided', () => {
    renderWithMantine(<OperatorStatusCard operator={{ ...operator, reason: '' }} pinned={false} />);
    expect(screen.getByText('No current disruption reported.')).toBeInTheDocument();
  });

  it('renders the sample summary with delay and cancellation stats', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    expect(screen.getByText(/Avg delay 7\.3 min/)).toBeInTheDocument();
    expect(screen.getByText(/10% cancelled/)).toBeInTheDocument();
  });

  it('renders a last-updated indicator', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    expect(screen.getByText(/Updated (just now|\d+[mhd] ago)/)).toBeInTheDocument();
  });

  it('renders the TfL hedge when sampleStats is undefined and code is TfL', () => {
    const tflOperator: OperatorSummary = {
      code: 'TfL',
      name: 'Transport for London',
      lineIds: ['elizabeth'],
      worstSeverity: 10,
      reason: '',
      sampleStats: undefined,
      computedAt: '2026-07-15T09:00:00Z',
    };
    renderWithMantine(<OperatorStatusCard operator={tflOperator} pinned={false} />);
    expect(screen.getByText("Not measured by this app — status is TfL's own.")).toBeInTheDocument();
  });

  it('renders the pinned state correctly when pinned is true', () => {
    const { container } = renderWithMantine(<OperatorStatusCard operator={operator} pinned={true} />);
    const pinToggle = container.querySelector('[aria-label*="Unpin"]');
    expect(pinToggle).toBeInTheDocument();
    expect(pinToggle).toHaveAttribute('aria-label', 'Unpin (currently pinned)');
  });

  it('renders the unpinned state correctly when pinned is false', () => {
    const { container } = renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    const pinToggle = container.querySelector('[aria-label*="Pin"]');
    expect(pinToggle).toBeInTheDocument();
    expect(pinToggle).toHaveAttribute('aria-label', 'Pin (currently not pinned)');
  });

  it('renders the account hint when needsAccountHint is true and pinned is false', () => {
    const { container } = renderWithMantine(
      <OperatorStatusCard operator={operator} pinned={false} needsAccountHint={true} />
    );
    const pinToggle = container.querySelector('[aria-label*="Pin"]');
    expect(pinToggle).toHaveAttribute('aria-label', 'Pin — needs an account');
  });

  it('clamps a long reason rather than letting it fill the card', () => {
    const wall = 'Station improvement work: '.repeat(40);
    const { container } = renderWithMantine(
      <OperatorStatusCard operator={{ ...operator, reason: wall }} pinned={false} />
    );
    const reason = container.querySelector('[data-card-reason]') as HTMLElement;
    expect(reason.style.getPropertyValue('-webkit-line-clamp')).toBe('3');
  });

  it('keeps the status badge on the title row even when the name wraps', () => {
    const { container } = renderWithMantine(
      <OperatorStatusCard operator={{ ...operator, name: 'A Very Long Operator Name That Might Wrap' }} pinned={false} />
    );
    const titleRow = container.querySelector('[data-card-title-row]') as HTMLElement;
    expect(titleRow).toBeInTheDocument();
  });

  it('renders no last-updated indicator when computedAt is null', () => {
    renderWithMantine(
      <OperatorStatusCard operator={{ ...operator, computedAt: null }} pinned={false} />
    );
    const updatedElements = screen.queryAllByText(/Updated/);
    expect(updatedElements).toHaveLength(0);
  });

  it('anchors the footer row to the card bottom so pin stars align across a row (review M6)', () => {
    const { container } = renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    const footer = container.querySelector('[data-card-footer]') as HTMLElement;
    expect(footer).toBeInTheDocument();
    expect(footer.style.marginTop).toBe('auto');
  });

  it('hides the pin star when showPin is false (review M9)', () => {
    const { container } = renderWithMantine(
      <OperatorStatusCard operator={operator} pinned={false} showPin={false} />
    );
    expect(container.querySelector('[aria-label*="Pin"]')).not.toBeInTheDocument();
  });

  it('shows the pin star by default', () => {
    const { container } = renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    expect(container.querySelector('[aria-label*="Pin"]')).toBeInTheDocument();
  });

  describe('rollup scope line (review I11)', () => {
    it('says which line is driving a bad status, linked to it', () => {
      renderWithMantine(
        <OperatorStatusCard
          operator={{ ...operator, worstLineId: 'ecml', worstLineName: 'East Coast Main Line' }}
          pinned={false}
        />
      );
      expect(screen.getByText(/Worst of 2 lines/)).toBeInTheDocument();
      const link = screen.getByText('East Coast Main Line').closest('a');
      expect(link).toHaveAttribute('href', '/lines/ecml');
    });

    it('falls back to an unlinked scope line when worstLineId/worstLineName are absent', () => {
      renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
      expect(screen.getByText(/Worst of 2 lines/)).toBeInTheDocument();
    });

    it('says "all running normally" instead of "Worst of" when the worst severity is Good Service', () => {
      renderWithMantine(
        <OperatorStatusCard operator={{ ...operator, worstSeverity: 10 }} pinned={false} />
      );
      expect(screen.getByText('2 lines, all running normally')).toBeInTheDocument();
      expect(screen.queryByText(/Worst of/)).not.toBeInTheDocument();
    });

    it('uses singular "line" for a single-line operator', () => {
      renderWithMantine(
        <OperatorStatusCard
          operator={{ ...operator, worstSeverity: 10, lineIds: ['ecml'] }}
          pinned={false}
        />
      );
      expect(screen.getByText('1 line, all running normally')).toBeInTheDocument();
    });
  });

  describe('deduped reason (review M10)', () => {
    it('collapses the reason into a cross-reference when dedupedLineId matches the worst line', () => {
      renderWithMantine(
        <OperatorStatusCard
          operator={{ ...operator, worstLineId: 'ecml', worstLineName: 'East Coast Main Line' }}
          pinned={false}
          dedupedLineId="ecml"
        />
      );
      expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
      expect(screen.getByText(/See/)).toBeInTheDocument();
      const link = screen.getByText('East Coast Main Line').closest('a');
      expect(link).toHaveAttribute('href', '/lines/ecml');
    });

    it('renders the ordinary reason when dedupedLineId does not match the worst line', () => {
      renderWithMantine(
        <OperatorStatusCard
          operator={{ ...operator, worstLineId: 'ecml', worstLineName: 'East Coast Main Line' }}
          pinned={false}
          dedupedLineId="wcml"
        />
      );
      expect(screen.getByText('Signal failure')).toBeInTheDocument();
    });
  });
});
