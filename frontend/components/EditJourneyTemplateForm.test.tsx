import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { EditJourneyTemplateForm } from './EditJourneyTemplateForm';
import type { JourneyTemplateDetail } from '@/lib/types';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock, push: vi.fn() }),
  usePathname: () => '/journeys/templates/167',
  useSearchParams: () => new URLSearchParams(''),
}));

function template(overrides: Partial<JourneyTemplateDetail> = {}): JourneyTemplateDetail {
  return {
    id: 167,
    customName: 'My commute',
    createdAt: '2026-09-22T00:00:00Z',
    updatedAt: '2026-09-22T00:00:00Z',
    daysOfWeek: null,
    active: true,
    startsOn: null,
    endsOn: null,
    defaultMatchMode: 'manual',
    autoCommitRule: null,
    legs: [
      {
        id: 1,
        originCrs: 'KGX',
        originName: 'London Kings Cross',
        destinationCrs: 'EDB',
        destinationName: 'Edinburgh',
        departAfter: '09:00',
        departBefore: '10:00',
        arriveAfter: null,
        arriveBefore: null,
      },
    ],
    ...overrides,
  };
}

describe('EditJourneyTemplateForm', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('pre-fills the template name and every leg field from the template', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    expect(screen.getByLabelText('Template name')).toHaveValue('My commute');
    expect(screen.getByLabelText('Origin CRS')).toHaveValue('KGX');
    expect(screen.getByLabelText('Destination CRS')).toHaveValue('EDB');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('09:00');
    expect(screen.getByLabelText('Latest departure (optional)')).toHaveValue('10:00');
  });

  it('adds another leg client-side, with no remove control while only one leg exists', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    expect(screen.getByText('Leg 1')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove leg 1' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    expect(screen.getByText('Leg 2')).toBeInTheDocument();
    expect(screen.getAllByLabelText('Origin CRS')).toHaveLength(2);
    // Now that there's more than one leg, a remove control appears -- a
    // plain text-labeled Button, not an icon (no @tabler/icons-react in
    // this project).
    expect(screen.getByRole('button', { name: 'Remove leg 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove leg 2' })).toBeInTheDocument();
  });

  it('removes a leg client-side via the plain-text Remove button', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));
    expect(screen.getAllByLabelText('Origin CRS')).toHaveLength(2);

    fireEvent.click(screen.getByRole('button', { name: 'Remove leg 2' }));

    expect(screen.getAllByLabelText('Origin CRS')).toHaveLength(1);
    expect(screen.queryByText('Leg 2')).not.toBeInTheDocument();
    // Back down to one leg -- the remove control disappears again.
    expect(screen.queryByRole('button', { name: 'Remove leg 1' })).not.toBeInTheDocument();
  });

  it('disables Save when any leg is missing an origin or destination', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    // The newly-added second leg has no origin/destination yet.
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();

    const originFields = screen.getAllByLabelText('Origin CRS');
    const destinationFields = screen.getAllByLabelText('Destination CRS');
    fireEvent.change(originFields[1], { target: { value: 'YRK' } });
    fireEvent.change(destinationFields[1], { target: { value: 'NCL' } });

    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();
  });

  it('PUTs the whole leg list, shows "Saved.", and calls router.refresh() on success', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/JourneyTemplates/167',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify({
            customName: 'My commute',
            legs: [
              {
                originCrs: 'KGX',
                destinationCrs: 'EDB',
                departWindow: { after: '09:00', before: '10:00' },
                arriveWindow: { after: null, before: null },
              },
            ],
          }),
        }),
      );
    });

    expect(await screen.findByText('Saved.')).toBeInTheDocument();
    expect(refreshMock).toHaveBeenCalled();
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to save changes' });
    expect(loginLink).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(screen.queryByText('Saved.')).not.toBeInTheDocument();
  });

  it('shows the server error message on a non-401 failure', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('destination must differ from origin', { status: 400 }));

    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(await screen.findByText('destination must differ from origin')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
