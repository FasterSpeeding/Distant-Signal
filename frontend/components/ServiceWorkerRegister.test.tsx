import { describe, it, expect, vi, afterEach } from 'vitest';
import { waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { theme } from '@/lib/theme';
import { ServiceWorkerRegister } from './ServiceWorkerRegister';

const originalServiceWorker = Object.getOwnPropertyDescriptor(navigator, 'serviceWorker');

describe('ServiceWorkerRegister', () => {
  afterEach(() => {
    if (originalServiceWorker) {
      Object.defineProperty(navigator, 'serviceWorker', originalServiceWorker);
    } else {
      delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    }
    window.localStorage.clear();
  });

  it('renders nothing', () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    const { container } = renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);
    expect(container.querySelectorAll('*:not(style)')).toHaveLength(0);
  });

  it('registers /sw.js with no explicit scope when serviceWorker is supported', async () => {
    const register = vi.fn().mockResolvedValue({});
    Object.defineProperty(navigator, 'serviceWorker', { value: { register }, configurable: true });

    renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);

    await waitFor(() => expect(register).toHaveBeenCalledWith('/sw.js'));
    expect(register).toHaveBeenCalledTimes(1);
  });

  it('does nothing (no throw) when serviceWorker is unsupported', () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    expect(() => renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />)).not.toThrow();
  });

  it('swallows a register() rejection without throwing', async () => {
    const register = vi.fn().mockRejectedValue(new Error('registration failed'));
    Object.defineProperty(navigator, 'serviceWorker', { value: { register }, configurable: true });

    expect(() => renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />)).not.toThrow();
    await waitFor(() => expect(register).toHaveBeenCalled());
  });

  it('writes loadedAt to localStorage["lastSuccessfulLoadAt"] on mount', async () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);
    await waitFor(() => expect(window.localStorage.getItem('lastSuccessfulLoadAt')).toBe('2026-09-02T10:00:00.000Z'));
  });

  it('updates localStorage again when loadedAt changes on a later render (a fresh successful navigation/refresh)', async () => {
    delete (navigator as unknown as Record<string, unknown>).serviceWorker;
    const { rerender } = renderWithMantine(<ServiceWorkerRegister loadedAt="2026-09-02T10:00:00.000Z" />);
    await waitFor(() => expect(window.localStorage.getItem('lastSuccessfulLoadAt')).toBe('2026-09-02T10:00:00.000Z'));

    rerender(
      <MantineProvider theme={theme}>
        <ServiceWorkerRegister loadedAt="2026-09-02T10:00:30.000Z" />
      </MantineProvider>,
    );
    await waitFor(() => expect(window.localStorage.getItem('lastSuccessfulLoadAt')).toBe('2026-09-02T10:00:30.000Z'));
  });
});
