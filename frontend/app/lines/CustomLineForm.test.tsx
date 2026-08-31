import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, act } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CustomLineForm } from './CustomLineForm';
import type { CustomLineDetail } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
}));

function renderWithProvider(props: { cancelHref?: string; existingLine?: CustomLineDetail } = {}) {
  return renderWithMantine(<CustomLineForm {...props} />);
}

const existingLine: CustomLineDetail = {
  id: 'my-line',
  name: 'My line',
  operators: [],
  stations: ['WOK', 'CLJ'],
  headcodePrefixes: [],
  destinationCrsFilter: [],
  isOwner: true,
};

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

  it('the committed operator pill carries the full name as a title tooltip', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Operators' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'sw' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    const option = await screen.findByRole('option', { name: 'SW — South Western Railway', hidden: true });
    fireEvent.click(option);

    const pill = screen.getByText('SW').closest('[title]');
    expect(pill).toHaveAttribute('title', 'South Western Railway');
  });

  it('renders no Cancel action when no cancelHref is given', () => {
    renderWithProvider();

    expect(screen.queryByRole('link', { name: 'Cancel' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create line' })).toBeInTheDocument();
  });

  it('places Cancel before the submit action in a single row when cancelHref is given', () => {
    renderWithProvider({ existingLine, cancelHref: '/lines/my-line' });

    const cancel = screen.getByRole('link', { name: 'Cancel' });
    const submit = screen.getByRole('button', { name: 'Save changes' });
    expect(cancel).toHaveAttribute('href', '/lines/my-line');
    // Both actions must share one parent row, and Cancel must come first —
    // the bug was Cancel rendering in a separate block *below* a
    // full-width submit button.
    expect(cancel.parentElement).toBe(submit.parentElement);
    expect(cancel.compareDocumentPosition(submit) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('Cancel does not submit the form', () => {
    renderWithProvider({ existingLine, cancelHref: '/lines/my-line' });

    fireEvent.click(screen.getByRole('link', { name: 'Cancel' }));

    const calls = (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mock.calls;
    expect(calls.some(([url]) => String(url).startsWith('/api/lines'))).toBe(false);
  });

  // `/lines/[id]/page.tsx` only links to this form's edit mode for the
  // line's owner, so a 401 here can only come from a session that lapses
  // between page load and this submit. Same `needsLogin` treatment as
  // `PinToggle`: a login prompt, never the raw backend rejection text
  // ("no session") this used to render straight into a red <Text>.
  it('a 401 on save shows a login prompt instead of the raw backend error text', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    renderWithProvider({ existingLine });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to edit a line' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login');
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
  });

  it('a 401 on create shows a login prompt worded for creating, not editing', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    renderWithProvider();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'My Commute' } });
    const stationInput = screen.getByRole('combobox', { name: 'Add station (CRS code)' });
    fireEvent.change(stationInput, { target: { value: 'WOK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.change(stationInput, { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.click(screen.getByRole('button', { name: 'Create line' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to create a line' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login');
  });

  // Every other non-ok status keeps the old behaviour -- only a 401 is
  // treated as "you need to log in".
  it('a non-401 failure shows the raw backend error text, not a login prompt', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('a line needs at least 2 stations', { status: 400 });
      }
      return new Response('[]', { status: 200 });
    });

    renderWithProvider({ existingLine });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(await screen.findByText('a line needs at least 2 stations')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Log in to/ })).not.toBeInTheDocument();
  });
});
