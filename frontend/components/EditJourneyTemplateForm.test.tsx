import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { EditJourneyTemplateForm } from './EditJourneyTemplateForm';
import type { JourneyTemplateDetail } from '@/lib/types';

/** Forces `input.validity.badInput`, which is how a real browser reports a
 * half-entered time ("09:--"). jsdom models neither segment state nor
 * `badInput`, so the only way to exercise that branch here is to say so
 * directly -- same helper as `TimeFilterInput.test.tsx`'s own. */
function setBadInput(input: HTMLInputElement, badInput: boolean) {
  Object.defineProperty(input, 'validity', {
    configurable: true,
    get: () => ({ badInput }),
  });
}

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock, push: vi.fn() }),
  usePathname: () => '/journeys/templates/167',
  useSearchParams: () => new URLSearchParams(''),
}));

const STATIONS = [
  { code: 'YRK', name: 'York' },
  { code: 'NCL', name: 'Newcastle' },
  { code: 'WOK', name: 'Woking' },
  { code: 'KGX', name: 'London Kings Cross' },
  { code: 'EDB', name: 'Edinburgh' },
];

/** Routes the mocked `fetch` by URL: the station fields' own type-ahead
 * search (`/api/stations?q=`, matched on code or name like the real API)
 * gets the `STATIONS` fixture, and anything else (the template PUT) gets
 * `put()`. */
function mockFetch(put: () => Response = () => new Response(null, { status: 204 })) {
  const fetchMock = vi.mocked(fetch);
  fetchMock.mockImplementation((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/stations?')) {
      const q = (new URLSearchParams(url.split('?')[1]).get('q') ?? '').toLowerCase();
      const matches = STATIONS.filter((s) => s.code.toLowerCase() === q || s.name.toLowerCase().includes(q));
      return Promise.resolve(new Response(JSON.stringify(matches), { status: 200 }));
    }
    return Promise.resolve(put());
  });
  return fetchMock;
}

/** The template PUTs made so far (station searches left out). */
function putCalls() {
  return vi.mocked(fetch).mock.calls.filter(([input]) => String(input).startsWith('/api/JourneyTemplates/'));
}

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
    mockFetch();
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('pre-fills the template name and every leg field from the template', async () => {
    mockFetch();
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    expect(screen.getByLabelText('Template name')).toHaveValue('My commute');
    // The saved codes, then their stations by name once looked up.
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'From station' })).toHaveValue('London Kings Cross (KGX)'),
    );
    expect(screen.getByRole('combobox', { name: 'To station' })).toHaveValue('Edinburgh (EDB)');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('09:00');
    expect(screen.getByLabelText('Latest departure (optional)')).toHaveValue('10:00');
  });

  it('adds another leg client-side, with no remove control while only one leg exists', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    expect(screen.getByText('Leg 1')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove leg 1' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    expect(screen.getByText('Leg 2')).toBeInTheDocument();
    expect(screen.getAllByRole('combobox', { name: 'From station' })).toHaveLength(2);
    // Now that there's more than one leg, a remove control appears -- a
    // plain text-labeled Button, not an icon (no @tabler/icons-react in
    // this project).
    expect(screen.getByRole('button', { name: 'Remove leg 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove leg 2' })).toBeInTheDocument();
  });

  it('removes a leg client-side via the plain-text Remove button', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));
    expect(screen.getAllByRole('combobox', { name: 'From station' })).toHaveLength(2);

    fireEvent.click(screen.getByRole('button', { name: 'Remove leg 2' }));

    expect(screen.getAllByRole('combobox', { name: 'From station' })).toHaveLength(1);
    expect(screen.queryByText('Leg 2')).not.toBeInTheDocument();
    // Back down to one leg -- the remove control disappears again.
    expect(screen.queryByRole('button', { name: 'Remove leg 1' })).not.toBeInTheDocument();
  });

  it('disables Save when any leg is missing an origin or destination', () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    // The newly-added second leg has no origin/destination yet.
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();

    const originFields = screen.getAllByRole('combobox', { name: 'From station' });
    const destinationFields = screen.getAllByRole('combobox', { name: 'To station' });
    fireEvent.change(originFields[1]!, { target: { value: 'YRK' } });
    fireEvent.change(destinationFields[1]!, { target: { value: 'NCL' } });

    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();
  });

  it("uses the site's station search for each leg's From and To, with the same placeholders as adding a leg", () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    const from = screen.getByRole('combobox', { name: 'From station' });
    const to = screen.getByRole('combobox', { name: 'To station' });
    expect(from).toHaveAttribute('placeholder', 'e.g. Woking or WOK');
    expect(to).toHaveAttribute('placeholder', 'e.g. London Waterloo or WAT');
  });

  it('PUTs the CRS code for a station typed by its full name', async () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    const origin = screen.getAllByRole('combobox', { name: 'From station' })[1]!;
    const destination = screen.getAllByRole('combobox', { name: 'To station' })[1]!;
    fireEvent.change(origin, { target: { value: 'York' } });
    fireEvent.blur(origin);
    fireEvent.change(destination, { target: { value: 'ncl' } });
    fireEvent.blur(destination);

    // Save waits for "York" to resolve through the station search, which
    // then shows the station by name and code.
    // Picked once the search for what was typed comes back.
    await waitFor(() => expect(origin).toHaveValue('York (YRK)'), { timeout: 5000 });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled());
    expect(screen.queryByText('Must be a 3-letter station code')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(putCalls()).toHaveLength(1));
    const body = JSON.parse(putCalls()[0]![1]?.body as string);
    expect(body.legs[1]).toMatchObject({ originCrs: 'YRK', destinationCrs: 'NCL' });
  });

  it('fills in the code when a station is picked from the suggestions by name', async () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    const origin = screen.getByRole('combobox', { name: 'From station' });
    fireEvent.change(origin, { target: { value: 'wok' } });
    fireEvent.click(await screen.findByRole('option', { name: 'WOK · Woking', hidden: true }));

    expect(origin).toHaveValue('Woking (WOK)');
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(putCalls()).toHaveLength(1));
    const body = JSON.parse(putCalls()[0]![1]?.body as string);
    expect(body.legs[0]).toMatchObject({ originCrs: 'WOK', destinationCrs: 'EDB' });
  });

  it('says when a typed value is not a station, once the field loses focus, and blocks Save', async () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    const origin = screen.getByRole('combobox', { name: 'From station' });
    fireEvent.change(origin, { target: { value: 'Nowhere at all' } });
    // Not while it is still being typed.
    expect(screen.queryByText('Must be a 3-letter station code')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();

    fireEvent.blur(origin);
    expect(await screen.findByText('Must be a 3-letter station code')).toBeInTheDocument();
    expect(origin).toHaveAttribute('aria-invalid', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(putCalls()).toHaveLength(0);
  });

  it('uppercases a station code typed in lower case', async () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'To station' }), { target: { value: 'yrk' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(putCalls()).toHaveLength(1));
    const body = JSON.parse(putCalls()[0]![1]?.body as string);
    expect(body.legs[0]).toMatchObject({ originCrs: 'KGX', destinationCrs: 'YRK' });
  });

  it('PUTs the whole leg list, shows "Saved.", and calls router.refresh() on success', async () => {
    const fetchMock = vi.mocked(fetch);

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

    const saved = await screen.findByText('Saved.');
    // Success is green app-wide (docs/style-guide.md "Semantic roles").
    expect(saved.closest<HTMLElement>('.mantine-Alert-root')?.style.getPropertyValue('--alert-color')).toContain(
      '--mantine-color-green-',
    );
    expect(refreshMock).toHaveBeenCalled();
  });

  it('a leg with no time fields at all is valid and PUTs every window bound as null (backend allows a fully-open-window template leg -- see api::data::journey_templates::validate_template_leg)', async () => {
    const fetchMock = vi.mocked(fetch);

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
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add another leg' }));

    const originFields = screen.getAllByRole('combobox', { name: 'From station' });
    const destinationFields = screen.getAllByRole('combobox', { name: 'To station' });
    fireEvent.change(originFields[1]!, { target: { value: 'YRK' } });
    fireEvent.change(destinationFields[1]!, { target: { value: 'NCL' } });

    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(putCalls()).toHaveLength(1));
    const body = JSON.parse(putCalls()[0]![1]?.body as string);
    expect(body.legs[1]).toEqual({
      originCrs: 'YRK',
      destinationCrs: 'NCL',
      departWindow: { after: null, before: null },
      arriveWindow: { after: null, before: null },
    });
  });

  // Bug: every `TimeFilterInput` here was wired with `onIncompleteChange={()
  // => {}}`, discarding the "this time field is half-entered" signal that
  // `TrainSearchForm`/`TrackTrainForm`/`AddJourneyLegButton` all correctly
  // guard on elsewhere in this codebase. A user who typed an hour, never
  // finished the minutes, and pressed Save got `departWindow.after: null`
  // PUT to the backend with a green "Saved." and no error -- the bound was
  // dropped silently. `setBadInput` + `fireEvent.change` with a partial
  // value is what triggers `validity.badInput` in jsdom, the same signal
  // `TimeFilterInput`'s own doc comment describes and its own test file
  // exercises the same way.
  it('a half-typed time on a leg blocks Save, rather than silently dropping the bound', async () => {
    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();

    // An hour with no minutes -- a native `<input type="time">` reports
    // `value` as `''` here (indistinguishable from untouched) but sets
    // `validity.badInput`, which is what `TimeFilterInput` actually reads.
    const departFrom = screen.getByLabelText<HTMLInputElement>('Earliest departure (optional)');
    setBadInput(departFrom, true);
    fireEvent.change(departFrom, { target: { value: '' } });

    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(putCalls()).toHaveLength(0);
  });

  // Compounding bug: legs were keyed by array index, so removing a middle
  // leg could shift a `TimeFilterInput`'s incomplete-state onto the wrong
  // leg's data. Legs now carry a stable `key` (the backend leg id, or a
  // client-side counter for a brand-new leg) instead, so an incomplete-time
  // block on leg 2 must travel with leg 2's own data when leg 1 is removed,
  // not linger on whatever now occupies index 0.
  it('removing an earlier leg does not carry its incomplete-time block onto the remaining leg', async () => {
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
            {
              id: 2,
              originCrs: 'EDB',
              originName: 'Edinburgh',
              destinationCrs: 'INV',
              destinationName: 'Inverness',
              departAfter: '09:00',
              departBefore: null,
              arriveAfter: null,
              arriveBefore: null,
            },
          ],
        })}
      />,
    );

    // Leave leg 1's earliest-departure half-typed -- via an actual value
    // transition (real time -> cleared-with-badInput), same as
    // `TimeFilterInput.test.tsx`'s own incomplete-time tests: setting an
    // already-empty field to '' again is a no-op DOM-wise and would never
    // fire a change event to begin with.
    const departFromFields = screen.getAllByLabelText<HTMLInputElement>('Earliest departure (optional)');
    setBadInput(departFromFields[0]!, false);
    fireEvent.change(departFromFields[0]!, { target: { value: '08:00' } });
    setBadInput(departFromFields[0]!, true);
    fireEvent.change(departFromFields[0]!, { target: { value: '' } });
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();

    // Remove leg 1 -- the incomplete block belongs to leg 1's own identity,
    // not to "whatever is now at index 0" (leg 2, post-removal).
    fireEvent.click(screen.getByRole('button', { name: 'Remove leg 1' }));

    expect(screen.getByRole('button', { name: 'Save changes' })).not.toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(putCalls()).toHaveLength(1));
    const body = JSON.parse(putCalls()[0]![1]?.body as string);
    expect(body.legs).toEqual([
      {
        originCrs: 'EDB',
        destinationCrs: 'INV',
        departWindow: { after: '09:00', before: null },
        arriveWindow: { after: null, before: null },
      },
    ]);
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    mockFetch(() => new Response('no session', { status: 401 }));

    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to save changes' });
    expect(loginLink).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(screen.queryByText('Saved.')).not.toBeInTheDocument();
  });

  it('shows the server error message on a non-401 failure', async () => {
    mockFetch(() => new Response('destination must differ from origin', { status: 400 }));

    renderWithMantine(<EditJourneyTemplateForm template={template()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(await screen.findByText('Destination must differ from origin.')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  describe('recurrence controls', () => {
    function sentBody() {
      return JSON.parse(putCalls()[0]![1]?.body as string);
    }

    it('toggling Monday sends daysOfWeek: 1 (bit 0)', async () => {
      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('checkbox', { name: 'Mon' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      expect(sentBody().daysOfWeek).toBe(1);
    });

    it('toggling Wednesday sends daysOfWeek: 4 (bit 2)', async () => {
      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('checkbox', { name: 'Wed' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      expect(sentBody().daysOfWeek).toBe(4);
    });

    it('combines multiple selected days into one bitmask', async () => {
      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('checkbox', { name: 'Mon' }));
      fireEvent.click(screen.getByRole('checkbox', { name: 'Wed' }));
      fireEvent.click(screen.getByRole('checkbox', { name: 'Sun' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      // Mon (1) | Wed (4) | Sun (64) = 69.
      expect(sentBody().daysOfWeek).toBe(69);
    });

    it('deselecting every day sends daysOfWeek: null, not 0 or []', async () => {
      // Start from a template that already recurs on Monday, then deselect it.
      renderWithMantine(<EditJourneyTemplateForm template={template({ daysOfWeek: 1 })} />);
      expect(screen.getByRole('checkbox', { name: 'Mon' })).toBeChecked();

      fireEvent.click(screen.getByRole('checkbox', { name: 'Mon' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      expect(sentBody().daysOfWeek).toBeNull();
    });

    it('shows "Not recurring" when no day is selected, and switches away once one is', () => {
      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      expect(screen.getByText('Not recurring')).toBeInTheDocument();

      fireEvent.click(screen.getByRole('checkbox', { name: 'Fri' }));

      expect(screen.queryByText('Not recurring')).not.toBeInTheDocument();
    });

    it('switching to "auto" always sends autoCommitRule: nearest_to_now', async () => {
      renderWithMantine(<EditJourneyTemplateForm template={template()} />);
      fireEvent.click(screen.getByRole('radio', { name: 'Auto-commit for me' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      const body = sentBody();
      expect(body.defaultMatchMode).toBe('auto');
      expect(body.autoCommitRule).toBe('nearest_to_now');
    });

    it('switching back to "manual" sends autoCommitRule: null', async () => {
      renderWithMantine(
        <EditJourneyTemplateForm template={template({ defaultMatchMode: 'auto', autoCommitRule: 'nearest_to_now' })} />,
      );
      fireEvent.click(screen.getByRole('radio', { name: "Remind me, don't guess" }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      const body = sentBody();
      expect(body.defaultMatchMode).toBe('manual');
      expect(body.autoCommitRule).toBeNull();
    });

    it('the Paused switch sends the real active boolean, inverted', async () => {
      renderWithMantine(<EditJourneyTemplateForm template={template({ active: true })} />);
      fireEvent.click(screen.getByRole('switch', { name: /^Paused/ }));
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(putCalls()).toHaveLength(1));
      expect(sentBody().active).toBe(false);
    });
  });
});
