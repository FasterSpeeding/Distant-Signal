import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RecentlyCleared, RecentlyClearedList } from './RecentlyCleared';
import * as api from '@/lib/api';
import type { RecentlyClearedIncident } from '@/lib/types';

vi.mock('@/lib/api');

// 21:26 BST on 2 Oct 2026.
const NOW = Date.parse('2026-10-02T20:40:00Z');

const item = (overrides: Partial<RecentlyClearedIncident> = {}): RecentlyClearedIncident => ({
  incidentId: '0071A58B',
  summary: 'CLEARED: Residual disruption between Brockenhurst and Bournemouth',
  description: '<p>Disruption between Brockenhurst and Bournemouth has now ended.</p>',
  operators: ['SW'],
  affectedLines: [{ id: 'swr-south-west-main', name: 'South West Main Line' }],
  clearedAt: '2026-10-02T20:26:00Z',
  lastListedAt: '2026-10-02T20:38:00Z',
  expectedUntil: '2026-10-02T21:30:00Z',
  expectedUntilEndOfDay: false,
  ...overrides,
});

describe('RecentlyClearedList', () => {
  it('is hidden when nothing was cleared', () => {
    const { container } = renderWithMantine(<RecentlyClearedList items={[]} now={NOW} />);
    expect(container.querySelector('[data-recently-cleared]')).toBeNull();
  });

  it('shows the title, the clear time, the end it beat and the clearing text', () => {
    renderWithMantine(<RecentlyClearedList items={[item()]} now={NOW} />);
    const section = screen.getByRole('region', { name: 'Recently cleared' });
    expect(
      within(section).getByRole('link', { name: 'Residual disruption between Brockenhurst and Bournemouth' }),
    ).toHaveAttribute('href', '/incidents/0071A58B');
    expect(within(section).getByText('Cleared 21:26 · Was expected until 22:30')).toBeInTheDocument();
    expect(within(section).getByText(/has now ended/)).toBeInTheDocument();
    expect(within(section).queryByText(/South West Main Line/)).toBeNull();
  });

  it('names the lines when asked, and leaves out an end that has passed', () => {
    renderWithMantine(
      <RecentlyClearedList items={[item({ expectedUntil: '2026-10-02T20:00:00Z' })]} now={NOW} showLines />,
    );
    expect(screen.getByText('Cleared 21:26')).toBeInTheDocument();
    expect(screen.getByText('Lines: South West Main Line')).toBeInTheDocument();
  });

  it('says when the end of the day was expected', () => {
    renderWithMantine(
      <RecentlyClearedList items={[item({ expectedUntil: null, expectedUntilEndOfDay: true })]} now={NOW} />,
    );
    expect(screen.getByText('Cleared 21:26 · Was expected until the end of the day')).toBeInTheDocument();
  });

  it('sanitizes the clearing text', () => {
    renderWithMantine(
      <RecentlyClearedList items={[item({ description: '<p>Ended.</p><script>alert(1)</script>' })]} now={NOW} />,
    );
    expect(document.querySelector('[data-recently-cleared] script')).toBeNull();
  });
});

describe('RecentlyCleared', () => {
  beforeEach(() => {
    vi.mocked(api.getRecentlyCleared).mockReset();
  });

  it('fetches for its scope', async () => {
    vi.mocked(api.getRecentlyCleared).mockResolvedValue([item()]);
    renderWithMantine(await RecentlyCleared({ scope: { station: 'BCU' }, now: NOW, showLines: true }));
    expect(api.getRecentlyCleared).toHaveBeenCalledWith({ station: 'BCU' });
    expect(screen.getByText('Lines: South West Main Line')).toBeInTheDocument();
  });

  it('says it could not load rather than claiming there are none', async () => {
    vi.mocked(api.getRecentlyCleared).mockRejectedValue(new Error('boom'));
    renderWithMantine(await RecentlyCleared({ scope: {}, now: NOW }));
    expect(screen.getByRole('region', { name: 'Recently cleared' })).toBeInTheDocument();
    expect(screen.getByText("Couldn't load recently cleared incidents. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/boom/)).toBeNull();
  });
});
