import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import LineDetailPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineStatusReport, LineSummary, CustomLineDetail } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getLineStatus: vi.fn(), getCustomLine: vi.fn(), getLineDefinition: vi.fn(), getAllLines: vi.fn() };
});
// DeleteLineButton (rendered whenever Edit/Delete render) calls useRouter()
// from next/navigation, which throws outside a real Next.js App Router
// tree -- same workaround PinToggle.test.tsx/TicketPanel.test.tsx use.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  notFound: vi.fn(),
}));

function report(id: string, name: string): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['SW'],
    computedAt: '2026-08-31T09:00:00Z',
    lineStatuses: [],
  };
}

const lines: LineSummary[] = [
  { id: 'custom-my-commute', name: 'My Commute', category: 'custom', operators: ['SW'], source: 'custom' },
];

function customLine(overrides: Partial<CustomLineDetail> = {}): CustomLineDetail {
  return {
    id: 'custom-my-commute',
    name: 'My Commute',
    operators: ['SW'],
    stations: ['WOK', 'CLJ'],
    headcodePrefixes: [],
    destinationCrsFilter: [],
    isOwner: false,
    ...overrides,
  };
}

async function renderPage(id = 'custom-my-commute') {
  const element = await LineDetailPage({ params: Promise.resolve({ id }) });
  return renderWithMantine(element);
}

describe('LineDetailPage Edit/Delete visibility', () => {
  beforeEach(() => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
  });

  it('a catalogue line (getCustomLine 404s) never shows Edit/Delete', async () => {
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    await renderPage();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
  });

  it('a custom line the viewer does not own (isOwner: false) does not show Edit/Delete', async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: false }));
    await renderPage();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
  });

  it('a custom line the viewer owns (isOwner: true) shows Edit/Delete', async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: true }));
    await renderPage();
    expect(screen.getByRole('link', { name: 'Edit' })).toHaveAttribute('href', '/lines/custom-my-commute/edit');
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });
});
