import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, act, cleanup } from '@testing-library/react';
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
  isOwner: true,
  sharedWithGroups: [],
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

    // Task 3.4.4: the chip is now numbered by travel order -- the first
    // station added is "1 WOK", not a bare "WOK".
    expect(screen.getByText('1 WOK')).toBeInTheDocument();
    expect(input).toHaveValue('');
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
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'zzzzzz' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect(await screen.findByRole('option', { name: 'No matching stations', hidden: true })).toBeInTheDocument();
  });

  // Review §2.10 / WCAG 1.4.11 (Non-text Contrast): the chip's `CloseButton`
  // used to keep Mantine's default grey icon colour on the filled grape
  // `Badge` background, measuring 1.69:1 -- short of the 3:1 non-text
  // contrast minimum, and the only hard numeric WCAG failure in the whole
  // accessibility review. `c="white"` matches the badge's own label colour
  // (4.85:1 on grape-7). Asserted via the rendered inline style rather than
  // a computed-contrast check: jsdom doesn't paint or resolve CSS custom
  // properties, so the `color: var(--mantine-color-white)` style Mantine's
  // `c` prop emits is the only observable trace of the fix in this
  // environment.
  it('the station chip close button is explicitly white, not the default grey, for WCAG 1.4.11 contrast', async () => {
    renderWithProvider({ existingLine });

    const closeButton = screen.getByRole('button', { name: 'Remove WOK' });
    expect(closeButton).toHaveStyle({ color: 'var(--mantine-color-white)' });
  });

  it('the committed station chip carries the resolved name as a title tooltip', async () => {
    // The chip keeps its bare code (space is at a premium in this compact
    // list) but gains a `title=` tooltip carrying the full name -- the
    // same tactic the operator pill already uses above. `nameByCode`
    // already holds the answer from the suggestions fetch, so this is a
    // pure-frontend fix with no backend dependency.
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

    const chip = screen.getByText('1 WOK').closest('[title]');
    expect(chip).toHaveAttribute('title', 'Woking');
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

    expect(screen.getByText('1 WOK')).toBeInTheDocument();
    expect(input).toHaveValue('');
  });

  it('typing a raw CRS code directly still works, without any dropdown suggestion selected', async () => {
    renderWithProvider();
    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });

    fireEvent.change(input, { target: { value: 'wok' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(screen.getByText('1 WOK')).toBeInTheDocument();
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
    expect(screen.getAllByText('1 WOK')).toHaveLength(1);
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
  it('a 401 on save shows the login prompt modal instead of the raw backend error text', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    mockUsePathname.mockReturnValue('/lines/my-line/edit');
    renderWithProvider({ existingLine });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(await screen.findByText('Log in to edit a custom line.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines%2Fmy-line%2Fedit',
    );
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
  });

  it('a 401 on create shows a login prompt worded for creating, not editing', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    mockUsePathname.mockReturnValue('/lines/new');
    renderWithProvider();
    // `exact: false`: Task 3.4.4's `withAsterisk` makes the label's own
    // text "Name *", not a bare "Name".
    fireEvent.change(screen.getByLabelText('Name', { exact: false }), { target: { value: 'My Commute' } });
    const stationInput = screen.getByRole('combobox', { name: 'Add station (CRS code)' });
    fireEvent.change(stationInput, { target: { value: 'WOK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.change(stationInput, { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.click(screen.getByRole('button', { name: 'Create line' }));

    expect(await screen.findByText('Log in to create a custom line.')).toBeInTheDocument();
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
    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
  });

  // Task 3.4.3: neither `/lines/new` nor `/lines/[id]/edit` said what a
  // custom line even is -- this now renders once, from the shared form,
  // for both create and edit.
  it('explains what a custom line is, for both create and edit', () => {
    renderWithProvider();
    expect(screen.getByText(/A custom line groups any stations and operators you choose/)).toBeInTheDocument();

    cleanup();
    renderWithProvider({ existingLine });
    expect(screen.getByText(/A custom line groups any stations and operators you choose/)).toBeInTheDocument();
  });

  // Task 3.4.13: the account-needed hint only makes sense while creating --
  // reaching the edit form at all already means the viewer is signed in
  // (the route 404s a non-owner before this form ever renders).
  it('shows the account-needed hint only when creating, not editing', () => {
    renderWithProvider();
    expect(screen.getByText(/Creating a line needs a Distant Signal account/)).toBeInTheDocument();

    cleanup();
    renderWithProvider({ existingLine });
    expect(screen.queryByText(/Creating a line needs a Distant Signal account/)).not.toBeInTheDocument();
  });

  // Task 3.4.4: the station list had no empty state at all before the
  // first station was added.
  it('shows an empty-state hint before any station is added, and hides it once one is', () => {
    renderWithProvider();
    expect(screen.getByText('No stations yet — add at least two, in travel order.')).toBeInTheDocument();

    const input = screen.getByRole('combobox', { name: 'Add station (CRS code)' });
    fireEvent.change(input, { target: { value: 'wok' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(screen.queryByText('No stations yet — add at least two, in travel order.')).not.toBeInTheDocument();
  });

  // Task 3.4.4: Name is validated as required with no visual mark.
  it('marks the Name field as required with an asterisk', () => {
    renderWithProvider();
    // Mantine's `withAsterisk` renders the `*` as its own `aria-hidden`
    // node next to the label text, not appended to the label string
    // itself -- `getByText` still finds it (only `getByRole` excludes
    // `aria-hidden` content by default).
    expect(screen.getByText('*')).toBeInTheDocument();
  });

  // Task 3.4.11: the edit page had no way to delete a line at all -- only
  // the (separate) detail page did.
  it('offers a "Delete line…" link only when editing an existing line', () => {
    renderWithProvider();
    expect(screen.queryByRole('button', { name: 'Delete line…' })).not.toBeInTheDocument();

    cleanup();
    renderWithProvider({ existingLine });
    expect(screen.getByRole('button', { name: 'Delete line…' })).toBeInTheDocument();
  });

  it('opens the delete confirmation modal from the "Delete line…" link', async () => {
    renderWithProvider({ existingLine });
    fireEvent.click(screen.getByRole('button', { name: 'Delete line…' }));
    expect(await screen.findByText('Delete this line?')).toBeInTheDocument();
  });
});
