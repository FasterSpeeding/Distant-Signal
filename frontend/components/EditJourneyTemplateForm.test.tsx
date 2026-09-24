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
            daysOfWeek: null,
            active: true,
            startsOn: null,
            endsOn: null,
            defaultMatchMode: 'manual',
            autoCommitRule: null,
          }),
        }),
      );
    });

    expect(await screen.findByText('Saved.')).toBeInTheDocument();
    expect(refreshMock).toHaveBeenCalled();
  });

  it('a leg with no time fields at all is valid and PUTs every window bound as null (backend allows a fully-open-window template leg -- see api::data::journey_templates::validate_template_leg)', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(
      <EditJourneyTemplateForm
        template={template({
          legs: [
            {
              id: 1,
              originCrs: 'KGX',
              originName: 'London Kings Cross',
              destinationCrs: 'EDB',
              destinationName: 'Edinburgh',
              departAfter: null,
              departBefore: null,
              arriveAfter: null,
              arriveBefore: null,
            },
          ],
        })}
      />,
    );

    // No time field was ever filled in -- origin/destination alone (the
    // only fields validate_template_leg actually checks) are enough to
    // enable Save.
    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();

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
                departWindow: { after: null, before: null },
                arriveWindow: { after: null, before: null },
              },
            ],
            daysOfWeek: null,
            active: true,
            startsOn: null,
            endsOn: null,
            defaultMatchMode: 'manual',
            autoCommitRule: null,
          }),
        }),
      );
    });
    expect(await screen.findByText('Saved.')).toBeInTheDocument();
  });

  it('a newly-added leg (all four time fields blank by construction) is valid once origin/destination are filled in, with no time field required', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    const originFields = screen.getAllByLabelText('Origin CRS');
    const destinationFields = screen.getAllByLabelText('Destination CRS');
    fireEvent.change(originFields[1], { target: { value: 'YRK' } });
    fireEvent.change(destinationFields[1], { target: { value: 'NCL' } });

    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const body = JSON.parse(fetchMock.mock.calls[0][1]?.body as string);
    expect(body.legs[1]).toEqual({
      originCrs: 'YRK',
      destinationCrs: 'NCL',
      departWindow: { after: null, before: null },
      arriveWindow: { after: null, before: null },
    });
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

  describe('recurrence controls', () => {
    function sentBody(fetchMock: ReturnType<typeof vi.mocked<typeof fetch>>) {
      const call = fetchMock.mock.calls[0];
      return JSON.parse(call[1]?.body as string);
    }

    it('toggling Monday sends daysOfWeek: 1 (bit 0)', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('checkbox', { name: 'Mon' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      expect(sentBody(fetchMock).daysOfWeek).toBe(1);
    });

    it('toggling Wednesday sends daysOfWeek: 4 (bit 2)', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('checkbox', { name: 'Wed' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      expect(sentBody(fetchMock).daysOfWeek).toBe(4);
    });

    it('combines multiple selected days into one bitmask', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('checkbox', { name: 'Mon' }));
      fireEvent.click(screen.getByRole('checkbox', { name: 'Wed' }));
      fireEvent.click(screen.getByRole('checkbox', { name: 'Sun' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      // Mon (1) | Wed (4) | Sun (64) = 69.
      expect(sentBody(fetchMock).daysOfWeek).toBe(69);
    });

    it('deselecting every day sends daysOfWeek: null, not 0 or []', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      // Start from a template that already recurs on Monday, then deselect it.
      renderWithMantine(<EditJourneyTemplateForm template={template({ daysOfWeek: 1 })} />);
      expect(screen.getByRole('checkbox', { name: 'Mon' })).toBeChecked();

      fireEvent.click(screen.getByRole('checkbox', { name: 'Mon' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      expect(sentBody(fetchMock).daysOfWeek).toBeNull();
    });

    it('shows "Not recurring" when no day is selected, and switches away once one is', () => {
      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      expect(screen.getByText('Not recurring')).toBeInTheDocument();

      fireEvent.click(screen.getByRole('checkbox', { name: 'Fri' }));

      expect(screen.queryByText('Not recurring')).not.toBeInTheDocument();
    });

    it('switching to "auto" always sends autoCommitRule: nearest_to_now', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('radio', { name: 'Auto-commit for me' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      const body = sentBody(fetchMock);
      expect(body.defaultMatchMode).toBe('auto');
      expect(body.autoCommitRule).toBe('nearest_to_now');
    });

    it('switching back to "manual" sends autoCommitRule: null', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      renderWithMantine(
        <EditJourneyTemplateForm
          template={template({ defaultMatchMode: 'auto', autoCommitRule: 'nearest_to_now' })}
        />,
      );
      fireEvent.click(screen.getByRole('radio', { name: "Remind me, don't guess" }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      const body = sentBody(fetchMock);
      expect(body.defaultMatchMode).toBe('manual');
      expect(body.autoCommitRule).toBeNull();
    });

    it('the Paused switch sends the real active boolean, inverted', async () => {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

      renderWithMantine(<EditJourneyTemplateForm template={template({ active: true })} />);
      fireEvent.click(screen.getByRole('switch', { name: /^Paused/ }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      expect(sentBody(fetchMock).active).toBe(false);
    });
  });
});
