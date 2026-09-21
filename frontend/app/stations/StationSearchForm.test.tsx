import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, act } from '@testing-library/react';
import { startTransition as reactStartTransition } from 'react';
import { renderWithMantine } from '@/test/render';
import { StationSearchForm } from './StationSearchForm';

// Resolves (or is replaced) per-test to control how long a simulated
// navigation stays in flight.
let resolveNavigation: () => void = () => {};

const pushMock = vi.fn(() => {
  // Mirrors what Next's real `useRouter().push` does internally
  // (node_modules/next/dist/client/components/app-router.js dispatches
  // the navigation inside its own nested `startTransition`): the pending
  // window doesn't close until the target route's RSC payload resolves.
  // Composing that as a controllable deferred promise here lets the
  // pending-state tests drive the window open and shut deterministically,
  // without standing up a real Next server.
  reactStartTransition(async () => {
    await new Promise<void>((resolve) => {
      resolveNavigation = resolve;
    });
  });
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
}));

function renderWithProvider() {
  return renderWithMantine(<StationSearchForm />);
}

describe('StationSearchForm', () => {
  beforeEach(() => {
    pushMock.mockClear();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([{ code: 'WOK', name: 'Woking' }]), { status: 200 })),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('selecting a suggestion sets the field to just the CRS code, not "code — name"', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'wok' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    // Mantine's dropdown is present in the DOM but `display: none` under
    // jsdom (floating-ui's positioning never gets real layout info here),
    // so the option must be queried past Testing Library's default
    // visibility filter — `fireEvent.click` dispatches directly to the
    // node regardless of CSS visibility, so the click still reaches
    // Mantine's real selection handler.
    const option = await screen.findByRole('option', { name: 'WOK — Woking', hidden: true });
    fireEvent.click(option);

    expect(input).toHaveValue('WOK');
  });

  it('shows the matching option in the dropdown when searching by station name, not just by code', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });

    fireEvent.focus(input);
    // Typing the full station name -- rather than the CRS code -- must
    // still surface the option. The backend already filters `suggestions`
    // against both code and name, but Mantine's Autocomplete additionally
    // re-filters the dropdown client-side using each option's `label`
    // (which is deliberately set to the code, not the name, for selection
    // behavior). Without a passthrough `filter`, that re-filtering hides
    // this option since "woking" never appears in the label "WOK".
    fireEvent.change(input, { target: { value: 'Woking' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect(await screen.findByRole('option', { name: 'WOK — Woking', hidden: true })).toBeInTheDocument();
  });

  it('shows an accessible "no matches" option instead of hiding the listbox when a search matches nothing', async () => {
    // Mantine's `Autocomplete` has no `nothingFoundMessage` prop at all
    // (unlike Select/MultiSelect) -- it hides its whole `role="listbox"`
    // dropdown outright whenever `data` is empty, leaving an open combobox
    // (`aria-expanded="true"`) with no `option`/`group` child, which fails
    // axe's `aria-required-children`. `withNoMatchPlaceholder`
    // (`lib/autocompleteNoMatch.ts`) works around the missing prop by
    // swapping in a single inert `role="option"` placeholder whenever the
    // real suggestions list is empty.
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })));
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'zzzzzz' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect(await screen.findByRole('option', { name: 'No matching stations', hidden: true })).toBeInTheDocument();
  });

  // I2 (2026-09-17 whole-branch review): `withNoMatchPlaceholder` used to
  // apply unconditionally, so `Autocomplete`'s default `openOnFocus`
  // popped a dropdown falsely reading "No matching stations" before the
  // user had typed anything at all.
  it('does not show the "no matches" placeholder on focus of a blank, untouched field', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });

    fireEvent.focus(input);

    expect(screen.queryByRole('option', { name: 'No matching stations', hidden: true })).not.toBeInTheDocument();
  });

  // I2: the same placeholder used to flash during every in-flight search,
  // since `useSuggestions` holds `suggestions` at its previous (often
  // empty) value throughout the debounce and the fetch.
  it('does not show the "no matches" placeholder while a search is still in flight', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'zzzzzz' } });
    // Deliberately NOT advancing past the 250ms debounce -- the search
    // hasn't settled yet, so the placeholder must not appear.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });

    expect(screen.queryByRole('option', { name: 'No matching stations', hidden: true })).not.toBeInTheDocument();
  });

  it('clicking Look up after typing a station name (without picking the dropdown option) resolves to its CRS code', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'Woking' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Look up|Looking up/ }));
    });

    expect(pushMock).toHaveBeenCalledWith('/stations/WOK');

    // Close out the pending transition so it doesn't leak into later tests.
    await act(async () => {
      resolveNavigation();
    });
  });

  it('shows a user-facing pending state and disables the button while navigation is in flight', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station name or CRS code' });
    fireEvent.change(input, { target: { value: 'WOK' } });

    expect(screen.getByRole('button', { name: 'Look up' })).toBeEnabled();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Look up' }));
    });

    // Developer vocabulary ("Rendering...") is out; user-facing wording is
    // in, and the button stays disabled so a second click can't fire a
    // second navigation.
    const pendingButton = screen.getByRole('button', { name: 'Looking up…' });
    expect(pendingButton).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Look up' })).not.toBeInTheDocument();

    // The results area itself carries a real pending indicator too, not
    // just the button — several seconds of a static button is not enough
    // feedback for where the user is actually looking.
    expect(screen.getByRole('status')).toBeInTheDocument();

    await act(async () => {
      resolveNavigation();
    });

    expect(screen.getByRole('button', { name: 'Look up' })).toBeEnabled();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('does not enter the pending state for a blank CRS code', async () => {
    renderWithProvider();
    const button = screen.getByRole('button', { name: 'Look up' });
    expect(button).toBeDisabled();

    await act(async () => {
      fireEvent.click(button);
    });

    expect(pushMock).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Look up' })).toBeDisabled();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  // "Near me" -- see StationSearchForm's own doc comments on `NearbyState`/
  // `handleNearMe` for the design this exercises.
  describe('near me', () => {
    // `navigator.geolocation` doesn't exist in jsdom by default (confirmed
    // by reading jsdom's own Navigator implementation), which is exactly
    // the "unavailable" case one test below relies on -- every other test
    // here has to install it itself.
    function stubGeolocation(
      getCurrentPosition: (
        success: (position: unknown) => void,
        error: (err: { code: number }) => void,
      ) => void,
    ) {
      Object.defineProperty(globalThis.navigator, 'geolocation', {
        value: { getCurrentPosition },
        configurable: true,
      });
    }

    afterEach(() => {
      // `configurable: true` above makes this safe -- restores jsdom's own
      // geolocation-less baseline for the next test rather than leaking a
      // stub across files (`vi.unstubAllGlobals()` only undoes
      // `vi.stubGlobal`, not a direct `Object.defineProperty`).
      delete (globalThis.navigator as { geolocation?: unknown }).geolocation;
    });

    it('hides the button entirely when navigator.geolocation is unavailable', () => {
      renderWithProvider();
      expect(screen.queryByRole('button', { name: 'Use my location' })).not.toBeInTheDocument();
    });

    it('shows a loading state, then the nearest stations with distance, on success', async () => {
      stubGeolocation((success: (position: unknown) => void) => {
        success({ coords: { latitude: 51.3191, longitude: -0.561 } });
      });
      vi.stubGlobal(
        'fetch',
        vi.fn(async (url: string) => {
          expect(url).toContain('/api/stations/nearby?');
          expect(url).toContain('lat=51.3191');
          expect(url).toContain('lon=-0.561');
          return new Response(
            JSON.stringify([
              { code: 'WOK', name: 'Woking', distanceKm: 0.4 },
              { code: 'BSK', name: 'Basingstoke', distanceKm: 12.34 },
            ]),
            { status: 200 },
          );
        }),
      );
      renderWithProvider();

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Use my location' }));
      });

      const list = await screen.findByRole('list', { name: 'Nearby stations' });
      expect(list).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Woking (WOK) — 0.4 km' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Basingstoke (BSK) — 12.3 km' })).toBeInTheDocument();
    });

    it('navigates to the station page when a nearby result is chosen', async () => {
      stubGeolocation((success: (position: unknown) => void) => {
        success({ coords: { latitude: 51.3191, longitude: -0.561 } });
      });
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => new Response(JSON.stringify([{ code: 'WOK', name: 'Woking', distanceKm: 0.4 }]), { status: 200 })),
      );
      renderWithProvider();

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Use my location' }));
      });
      await act(async () => {
        fireEvent.click(await screen.findByRole('button', { name: 'Woking (WOK) — 0.4 km' }));
      });

      expect(pushMock).toHaveBeenCalledWith('/stations/WOK');

      await act(async () => {
        resolveNavigation();
      });
    });

    it('shows a calm, non-error message when the location prompt is declined', async () => {
      stubGeolocation((_success: (position: unknown) => void, error: (err: { code: number }) => void) => {
        error({ code: 1 });
      });
      renderWithProvider();

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Use my location' }));
      });

      expect(screen.getByText('Location access was declined. You can still search by name above.')).toBeInTheDocument();
      // Not rendered as an alarming/error state.
      expect(screen.queryByText("Couldn't find your location")).not.toBeInTheDocument();
    });

    it('shows an error state for a genuine geolocation failure other than permission denial', async () => {
      stubGeolocation((_success: (position: unknown) => void, error: (err: { code: number }) => void) => {
        error({ code: 2 }); // POSITION_UNAVAILABLE
      });
      renderWithProvider();

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Use my location' }));
      });

      expect(screen.getByText("Couldn't find your location")).toBeInTheDocument();
    });

    it('shows a loading indicator while the geolocation request is in flight', async () => {
      let deferredSuccess: (position: unknown) => void = () => {};
      stubGeolocation((success: (position: unknown) => void) => {
        deferredSuccess = success;
      });
      renderWithProvider();

      fireEvent.click(screen.getByRole('button', { name: 'Use my location' }));

      expect(screen.getByRole('button', { name: 'Locating…' })).toBeDisabled();
      expect(screen.getByText('Finding stations near you…')).toBeInTheDocument();

      vi.stubGlobal(
        'fetch',
        vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
      );
      await act(async () => {
        deferredSuccess({ coords: { latitude: 0, longitude: 0 } });
      });

      expect(screen.getByRole('button', { name: 'Use my location' })).toBeEnabled();
    });
  });
});
