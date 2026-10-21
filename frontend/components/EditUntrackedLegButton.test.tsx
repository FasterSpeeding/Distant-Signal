import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { EditUntrackedLegButton } from './EditUntrackedLegButton';
import type { JourneyLegDetail } from '@/lib/types';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/journeys/3',
  useSearchParams: () => new URLSearchParams(''),
}));

const leg: JourneyLegDetail = {
  id: 12,
  kind: 'untracked',
  originCrs: null,
  originName: 'Brixton',
  destinationCrs: null,
  destinationName: 'Euston',
  serviceDate: '2026-10-21',
  departAfter: null,
  departBefore: null,
  arriveAfter: null,
  arriveBefore: null,
  windowSearched: false,
  matchMode: 'manual',
  trackedTrainState: null,
  legSkip: null,
  transitMode: 'tube',
  lineName: 'Victoria line',
  plannedDeparture: '2026-10-21T08:05:00Z',
  plannedArrival: '2026-10-21T08:30:00Z',
  untrackedStatus: 'planned',
};

/** Spec 2026-10-20 §2.4 follow-up: an untracked leg is edited in place. */
describe('EditUntrackedLegButton', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(null, { status: 204 })),
    );
    refreshMock.mockClear();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('opens with every field the leg was made with, and an untouched save keeps them all, stop ids too', async () => {
    renderWithMantine(
      <EditUntrackedLegButton
        journeyId={3}
        leg={{ ...leg, originStopId: '940GZZLUBXN', destinationStopId: '940GZZLUEUS' }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Edit leg' }));
    expect(await screen.findByLabelText('Line (optional)')).toHaveValue('Victoria line');
    // The mode ("How", a Mantine Select showing its label).
    expect(screen.getByDisplayValue('Tube')).toHaveAttribute('id', screen.getByText('How').getAttribute('for'));
    expect(screen.getByRole('combobox', { name: 'From (optional)' })).toHaveValue('Brixton');
    expect(screen.getByRole('combobox', { name: 'To (optional)' })).toHaveValue('Euston');
    expect(screen.getByLabelText('Date')).toHaveValue('21 Oct 2026');
    expect(screen.getByLabelText('Leaves')).toHaveValue('09:05');
    expect(screen.getByLabelText('Arrives')).toHaveValue('09:30');

    fireEvent.click(screen.getByRole('button', { name: 'Save leg' }));
    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(
        '/api/Journeys/3/legs/12',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify({
            transitMode: 'tube',
            lineName: 'Victoria line',
            originName: 'Brixton',
            originStopId: '940GZZLUBXN',
            destinationName: 'Euston',
            destinationStopId: '940GZZLUEUS',
            plannedDeparture: '2026-10-21T08:05:00.000Z',
            plannedArrival: '2026-10-21T08:30:00.000Z',
          }),
        }),
      );
    });
  });

  it('opens seeded from the leg and PUTs the edited fields', async () => {
    renderWithMantine(<EditUntrackedLegButton journeyId={3} leg={leg} />);
    fireEvent.click(screen.getByRole('button', { name: 'Edit leg' }));
    expect(await screen.findByLabelText('Line (optional)')).toHaveValue('Victoria line');
    expect(screen.getByRole('combobox', { name: 'From (optional)' })).toHaveValue('Brixton');
    expect(screen.getByLabelText('Date')).toHaveValue('21 Oct 2026');
    expect(screen.getByLabelText('Leaves')).toHaveValue('09:05');
    expect(screen.getByLabelText('Arrives')).toHaveValue('09:30');

    fireEvent.change(screen.getByLabelText('Line (optional)'), { target: { value: 'Northern line' } });
    fireEvent.change(screen.getByLabelText('Arrives'), { target: { value: '09:40' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save leg' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(
        '/api/Journeys/3/legs/12',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify({
            transitMode: 'tube',
            lineName: 'Northern line',
            originName: 'Brixton',
            destinationName: 'Euston',
            plannedDeparture: '2026-10-21T08:05:00.000Z',
            plannedArrival: '2026-10-21T08:40:00.000Z',
          }),
        }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
