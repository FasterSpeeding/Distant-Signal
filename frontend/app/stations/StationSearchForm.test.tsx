import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { startTransition as reactStartTransition } from 'react';
import { MantineProvider } from '@mantine/core';
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
  return render(
    <MantineProvider>
      <StationSearchForm />
    </MantineProvider>,
  );
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
    const input = screen.getByRole('combobox', { name: 'Station CRS code' });

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

  it('shows a user-facing pending state and disables the button while navigation is in flight', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Station CRS code' });
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
});
