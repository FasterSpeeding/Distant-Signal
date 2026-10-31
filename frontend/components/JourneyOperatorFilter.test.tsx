import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyOperatorFilter } from './JourneyOperatorFilter';
import type { PlannerOperatorCatalogue } from '@/lib/types';

const refresh = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh }),
}));

const catalogue: PlannerOperatorCatalogue = {
  operators: [
    { code: 'GR', name: 'LNER' },
    { code: 'VT', name: 'Avanti West Coast' },
  ],
  groups: [],
  dsOperators: [],
};

/** `GET /api/Trips/operators` answers with the catalogue; the `PUT` with
 * `status`. */
function stubFetch(status = 204) {
  const fetchMock = vi.fn(async (url: string) =>
    url === '/api/Trips/operators'
      ? new Response(JSON.stringify(catalogue), { status: 200 })
      : new Response(status === 204 ? null : '', { status }),
  );
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
  refresh.mockReset();
});

describe('JourneyOperatorFilter', () => {
  it('names the filter from the catalogue and removes it', async () => {
    const fetchMock = stubFetch();
    renderWithMantine(
      <JourneyOperatorFilter journeyId={12} filter={{ mode: 'only', codes: ['VT'], entries: ['VT'] }} />,
    );
    expect(
      await screen.findByText('Planned using only Avanti West Coast. Suggestions after a disruption follow it.'),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/12/operator-filter', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ operatorFilter: null }),
    });
  });

  it('changes the mode with the planner field and saves it', async () => {
    const fetchMock = stubFetch();
    renderWithMantine(
      <JourneyOperatorFilter journeyId={12} filter={{ mode: 'only', codes: ['GR'], entries: ['GR'] }} />,
    );
    await screen.findByText(/Planned using only LNER/);
    fireEvent.click(screen.getByRole('button', { name: 'Change' }));
    fireEvent.click(screen.getByRole('radio', { name: 'Avoid these' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/12/operator-filter', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ operatorFilter: { mode: 'avoid', entries: ['GR'] } }),
    });
  });

  it('says so when saving fails, and names by code without the catalogue', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 500 })),
    );
    renderWithMantine(
      <JourneyOperatorFilter journeyId={12} filter={{ mode: 'avoid', codes: ['GR'], entries: ['GR'] }} />,
    );
    expect(screen.getByText(/Planned avoiding GR/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(await screen.findByText("Couldn't save the operator filter. Try again.")).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();
  });
});
