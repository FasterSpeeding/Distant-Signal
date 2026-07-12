import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { StationSearchForm } from './StationSearchForm';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
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
});
