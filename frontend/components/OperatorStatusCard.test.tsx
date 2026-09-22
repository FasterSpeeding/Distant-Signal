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
  // 2026-09-22 UX review, C2: `/operators/[code]/history` shipped with no
  // inbound href anywhere in the app.
  it('links to this operator\'s history page', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    const link = screen.getByRole('link', { name: 'History for Virgin Trains' });
    expect(link).toHaveAttribute('href', '/operators/VT/history');
  });

  it('gives the history link a per-operator accessible name, not a bare "History"', () => {
    renderWithMantine(<OperatorStatusCard operator={operator} pinned={false} />);
    // The visible text stays "History" (short, scannable, and repeated
    // down a grid of cards is fine visually); the accessible name carries
    // the operator so a screen-reader link list is not N identical items.
    expect(screen.getByText('History')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'History' })).not.toBeInTheDocument();
  });

  it('percent-encodes an operator code that needs it', () => {
    renderWithMantine(
      <OperatorStatusCard operator={{ ...operator, code: 'A/B' }} pinned={false} />
    );
    expect(screen.getByRole('link', { name: 'History for Virgin Trains' })).toHaveAttribute(
      'href',
      '/operators/A%2FB/history',
    );
  });
});
