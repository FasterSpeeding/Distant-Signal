import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, act, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CustomLineForm } from './CustomLineForm';
import type { CustomLineDetail } from '@/lib/types';

// The two 401 tests below render at two different real routes (`/lines`
// for creating, `/lines/[id]/edit` for editing, per app/lines/page.tsx and
// app/lines/[id]/edit/page.tsx) -- usePathname is a vi.fn() so each test can
// set its own value, rather than one static pathname standing in for both.
const mockUsePathname = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => mockUsePathname(),
  useSearchParams: () => new URLSearchParams(''),
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

  // Mirrors `StationSearchForm`'s own name-vs-code resolution test: typing
  // a full station name and selecting the dropdown suggestion for it adds
  // the CRS code the suggestion carries, not the raw typed text.
  it('typing a station name and selecting a suggestion adds the resolved CRS code as a pill', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'Woking' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    const option = await screen.findByRole('option', { name: 'WOK — Woking', hidden: true });
    fireEvent.click(option);
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(screen.getByText('WOK')).toBeInTheDocument();
    expect(input).toHaveValue('');
  });

  // Clicking Add after typing a station name -- without picking the
  // dropdown option first -- must still resolve to the right CRS code,
  // the same "Look up" resolution `StationSearchForm` uses: exact code
  // match, then exact name match, then best substring match, then raw
  // text as a last resort.
  it('clicking Add after typing a station name (without picking the dropdown option) resolves to its CRS code', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'Woking' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(screen.getByText('WOK')).toBeInTheDocument();
    expect(input).toHaveValue('');
  });

  it('typing a raw CRS code directly still works, without any dropdown suggestion selected', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.change(input, { target: { value: 'wok' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(screen.getByText('WOK')).toBeInTheDocument();
    expect(input).toHaveValue('');
  });

  // The existing dedup/length validation is the actual gate, applied to
  // whatever the autocomplete resolved -- it must still reject a duplicate
  // even when the duplicate was reached by typing a station name rather
  // than its raw code.
  it('does not add a duplicate station resolved from a station name', async () => {
    renderWithProvider({ existingLine });
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'Woking' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    // `existingLine.stations` already contains 'WOK' -- still exactly one
    // WOK badge, not two.
    expect(screen.getAllByText('WOK')).toHaveLength(1);
  });

  // Typed text that resolves (via the raw-text fallback) to something
  // other than a 3-letter code must still be rejected -- the autocomplete
  // doesn't bypass the length gate.
  it('does not add a station when the resolved text is not a valid 3-letter code', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.change(input, { target: { value: 'Nonexistent Station' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(screen.queryByText('NONEXISTENT STATION')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create line' })).toBeInTheDocument();
  });

  it('selecting an operator suggestion adds just the ATOC code as a tag', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Operators' });

    input.focus();
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

    input.focus();
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

    mockUsePathname.mockReturnValue('/lines/my-line/edit');
    renderWithProvider({ existingLine });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to edit a line' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Flines%2Fmy-line%2Fedit');
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
  });

  it('a 401 on create shows a login prompt worded for creating, not editing', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    mockUsePathname.mockReturnValue('/lines');
    renderWithProvider();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'My Commute' } });
    const stationInput = screen.getByRole('combobox', { name: 'Add station (CRS code)' });
    fireEvent.change(stationInput, { target: { value: 'WOK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.change(stationInput, { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.click(screen.getByRole('button', { name: 'Create line' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to create a line' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Flines');
  });

  // Reproduces the "Create line gets stuck loading" bug: creating a line
  // navigates back to `/lines`, the very route this form is already
  // rendered on (`app/lines/page.tsx`). A same-route `router.push` doesn't
  // remount this Client Component -- React reconciles it in place since its
  // type/position in the tree don't change -- so unlike the edit flow
  // (which always navigates to the different `/lines/{id}` route and gets
  // a fresh mount for free), nothing here ever reset `submitting` back to
  // `false` on the success path. Confirmed live against a running
  // dev stack: the new line appeared in the table immediately, but the
  // button below it kept its loading spinner forever. `useRouter().push`
  // is a no-op `vi.fn()` in this test file (see the top-of-file mock), the
  // same stand-in for "navigated to a route that doesn't remount me" a real
  // same-route push would produce.
  it('a successful create resets the submit button out of its loading state and clears the form', async () => {
    renderWithProvider();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'My Commute' } });
    const stationInput = screen.getByRole('combobox', { name: 'Add station (CRS code)' });
    fireEvent.change(stationInput, { target: { value: 'WOK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.change(stationInput, { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    const submit = screen.getByRole('button', { name: 'Create line' });
    fireEvent.click(submit);

    // Still submitting until the (mocked) fetch resolves.
    expect(submit).toHaveAttribute('data-loading', 'true');

    await waitFor(() => expect(submit).not.toHaveAttribute('data-loading', 'true'));
    expect(screen.getByLabelText('Name')).toHaveValue('');
    expect(screen.queryByText('WOK')).not.toBeInTheDocument();
    expect(screen.queryByText('CLJ')).not.toBeInTheDocument();
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
