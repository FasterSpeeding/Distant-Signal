import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { CustomLineForm } from './CustomLineForm';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
}));

function renderWithProvider() {
  return render(
    <MantineProvider>
      <CustomLineForm />
    </MantineProvider>,
  );
}

describe('CustomLineForm', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.includes('/api/stations')) {
          return new Response(JSON.stringify([{ code: 'WOK', name: 'Woking' }]), { status: 200 });
        }
        if (url.includes('/api/tocs')) {
          return new Response(JSON.stringify([{ code: 'SW', name: 'South Western Railway' }]), { status: 200 });
        }
        return new Response('[]', { status: 200 });
      }),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('selecting a station suggestion sets the Add station field to just the CRS code', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'wok' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    const option = await screen.findByRole('option', { name: 'WOK — Woking', hidden: true });
    fireEvent.click(option);

    expect(input).toHaveValue('WOK');
  });

  it('selecting an operator suggestion adds just the ATOC code as a tag', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Operators' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'sw' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    const option = await screen.findByRole('option', { name: 'SW — South Western Railway', hidden: true });
    fireEvent.click(option);

    expect(screen.getByText('SW')).toBeInTheDocument();
    expect(screen.queryByText('SW — South Western Railway')).not.toBeInTheDocument();
  });
});
