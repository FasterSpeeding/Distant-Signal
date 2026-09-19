import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TicketEntryForm } from './TicketEntryForm';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/train/by-id/1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('TicketEntryForm', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function openForm() {
    renderWithMantine(<TicketEntryForm trackingId={1} label="Add a ticket for this journey" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a ticket for this journey' }));
  }

  // Task 3.6.4: the manual-entry Operator/Origin/Destination fields are
  // now `Autocomplete`s backed by `searchStations`/`searchTocs`
  // (`/api/stations?q=`/`/api/tocs?q=`), same as `TrackTrainForm.tsx`'s
  // own fields -- debounced 250ms after every keystroke
  // (`lib/useSuggestions.ts`). Every test in this file shares one global
  // `fetch` mock configured for a SPECIFIC other endpoint (the submit/
  // upload route under test), so without this routing, a debounced
  // suggestion fetch would resolve against that same mocked `Response`
  // instance -- either racing a second `.json()`/`.text()` read of an
  // already-consumed body, or handing the Autocomplete's `.map()` a
  // non-array payload (e.g. `{ ticketId: 1 }`) mid-render. Routing these
  // two query-string prefixes to a fresh, inert empty array -- exactly
  // `TrackTrainForm.test.tsx`'s own `mockFetchByUrl` pattern -- removes
  // that hazard regardless of timing.
  function mockDefaultResponse(response: Response) {
    vi.mocked(fetch).mockImplementation((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith('/api/stations?') || url.startsWith('/api/tocs?')) {
        return Promise.resolve(new Response('[]', { status: 200 }));
      }
      return Promise.resolve(response);
    });
  }

  // Finds a recorded `fetch` call BY URL rather than by position --
  // `TrackTrainForm.test.tsx`'s own `trackCallBody` helper uses the same
  // approach for the same reason: with a debounced suggestion fetch now
  // also possibly in flight (see `mockDefaultResponse`'s own comment), the
  // submit/upload call under test is not reliably the first or the last
  // entry in `mock.calls`.
  function findFetchCall(url: string) {
    const call = vi.mocked(fetch).mock.calls.find((args) => args[0] === url);
    if (!call) throw new Error(`no ${url} call recorded`);
    return call as [string, RequestInit];
  }

  // Mantine's `FileInput` renders its visible, labelled element as a
  // `<button>` (see `InputBase`'s `component: 'button'`) and keeps the real
  // `<input type="file">` hidden and click-triggered, with no `id`/`for` or
  // `aria-labelledby` connecting it back to the visible label at all
  // (`FileButton.tsx` renders it as a bare, unlabelled `style={{ display:
  // 'none' }}` input). `screen.getByLabelText('Apple Wallet .pkpass file')`
  // therefore resolves to that visible button, not the real file input, and
  // `fireEvent.change` on a `<button>` is a no-op -- confirmed directly: with
  // the brief's exact `getByLabelText` call, `handleUpload`'s `fetch` was
  // never invoked at all. The two upload tabs' hidden inputs are told apart
  // by their distinct `accept` values (`.pkpass` vs `application/pdf`, per
  // `TicketEntryForm.tsx`'s own `UploadPanel` usage), since Mantine's `Tabs`
  // keeps every panel mounted (just `display: none`), so both hidden inputs
  // are present in the DOM regardless of which tab is active.
  function getPkpassFileInput(): HTMLInputElement {
    return document.querySelector('input[type="file"][accept=".pkpass"]') as HTMLInputElement;
  }

  // Same rationale as `getPkpassFileInput` above -- told apart from the
  // `.pkpass` tab's hidden input by its distinct `accept` value.
  function getPdfFileInput(): HTMLInputElement {
    return document.querySelector('input[type="file"][accept="application/pdf"]') as HTMLInputElement;
  }

  // Mirrors react-dropzone's own test helper (react-dropzone/src/index.spec.js,
  // createDtWithFiles) -- the shape react-dropzone@15.0.0's internal
  // onDrop/onDragEnter handlers actually read off a native DragEvent's
  // dataTransfer, confirmed against that file directly rather than guessed.
  function dropFiles(node: Element, files: File[]) {
    const dataTransfer = {
      files,
      items: files.map((file) => ({
        kind: 'file',
        size: file.size,
        type: file.type,
        getAsFile: () => file,
      })),
      types: ['Files'],
    };
    fireEvent.drop(node, { dataTransfer });
  }

  // The Dropzone's own focusable/drop-target root is an ANCESTOR of its
  // hidden file input, not found via `closest('[tabindex]')` as might be
  // assumed -- react-dropzone's hidden `<input>` itself carries
  // `tabindex="-1"` (confirmed against the real rendered DOM this
  // session), so that selector resolves to the input itself, not its
  // parent. The actual focusable/keyboard-activatable/drop-target root is
  // the input's immediate parent `<div>`, carrying `tabindex="0"`,
  // `role="presentation"`, and Mantine's own stable, non-hashed
  // `mantine-Dropzone-root` class (confirmed live via `npm run dev` this
  // session, per Task 3's verification) -- used here instead, since it's
  // stable across builds (unlike the CSS-module-hashed class alongside
  // it).
  function getPkpassDropzoneRoot(): HTMLElement {
    return getPkpassFileInput().closest('.mantine-Dropzone-root') as HTMLElement;
  }

  function getPdfDropzoneRoot(): HTMLElement {
    return getPdfFileInput().closest('.mantine-Dropzone-root') as HTMLElement;
  }

  // Task 3.6.6: the combined dropzone above the tabs accepts BOTH kinds at
  // once (`accept={[...PDF_MIME_TYPE, '.pkpass']}`), so its rendered
  // `accept` attribute is the two joined together -- distinct from either
  // dedicated tab's single-type input, which the two helpers above already
  // select by an EXACT attribute match.
  function getCombinedFileInput(): HTMLInputElement {
    return document.querySelector('input[type="file"][accept="application/pdf,.pkpass"]') as HTMLInputElement;
  }

  function getCombinedDropzoneRoot(): HTMLElement {
    return getCombinedFileInput().closest('.mantine-Dropzone-root') as HTMLElement;
  }

  it('starts collapsed, showing only the entry-point button', () => {
    renderWithMantine(<TicketEntryForm trackingId={1} label="Add a ticket for this journey" />);
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Operator (optional)' })).not.toBeInTheDocument();
  });

  it('defaultOpen renders the manual-entry tab immediately, with no collapsed-button click needed', () => {
    renderWithMantine(<TicketEntryForm label="Add a ticket" defaultOpen />);
    expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a ticket' })).not.toBeInTheDocument();
  });

  it('expands into the manual-entry tab by default when opened', () => {
    openForm();
    expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Manual', selected: true })).toBeInTheDocument();
  });

  it('manual submit: on success, saves, collapses, and refreshes the page', async () => {
    mockDefaultResponse(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
    openForm();
    fireEvent.change(screen.getByRole('combobox', { name: 'Operator (optional)' }), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(
        '/api/Train/1/tickets',
        expect.objectContaining({ method: 'POST' }),
      );
    });
    const [, init] = findFetchCall('/api/Train/1/tickets');
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({ operator: 'LNER', source: 'manual' });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
  });

  it('manual submit: on a 401, shows the login prompt modal and preserves typed fields', async () => {
    mockDefaultResponse(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.change(screen.getByRole('combobox', { name: 'Operator (optional)' }), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    expect(await screen.findByText('Log in to save this ticket.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrain%2Fby-id%2F1',
    );
    expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toHaveValue('LNER');
  });

  it('manual submit: on a 400, shows the backend message inline', async () => {
    // The exact copy is
    // `crates/api/src/data/train_tracking.rs::validate_ticket_entry`'s
    // source of truth -- this is testing the pass-through, not owning the
    // wording itself.
    mockDefaultResponse(
      new Response("That doesn't look like a station code — CRS codes are three letters, like WOK or EUS.", {
        status: 400,
      }),
    );
    openForm();
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));
    expect(
      await screen.findByText("That doesn't look like a station code — CRS codes are three letters, like WOK or EUS."),
    ).toBeInTheDocument();
  });

  it.each([
    [400, "That doesn't look like a valid upload — try again or fill in the form manually"],
    [422, 'could not read this as a train .pkpass: not a zip file'],
    [504, 'That file took too long to read — try a smaller or simpler PDF, or fill in the details manually'],
    [413, 'That file is too large (8 MB limit). Try filling in the details manually'],
    [500, "Couldn't read this file. Try filling in the details manually"],
  ])('pkpass upload: a %i response shows the mapped inline message', async (status, expectedSubstring) => {
    mockDefaultResponse(
      new Response(status === 422 ? 'could not read this as a train .pkpass: not a zip file' : 'error', { status }),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });

    expect(await screen.findByText(expectedSubstring)).toBeInTheDocument();
    // The manual form must stay reachable regardless of why the upload
    // failed.
    expect(screen.getByRole('button', { name: 'or fill in the details manually' })).toBeInTheDocument();
  });

  it.each([
    [400, "That doesn't look like a valid upload — try again or fill in the form manually"],
    [422, 'could not read this as a train .pkpass: not a zip file'],
    [504, 'That file took too long to read — try a smaller or simpler PDF, or fill in the details manually'],
    [413, 'That file is too large (8 MB limit). Try filling in the details manually'],
    [500, "Couldn't read this file. Try filling in the details manually"],
  ])('pkpass drop: a %i response shows the mapped inline message', async (status, expectedSubstring) => {
    mockDefaultResponse(
      new Response(status === 422 ? 'could not read this as a train .pkpass: not a zip file' : 'error', { status }),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    dropFiles(getPkpassDropzoneRoot(), [file]);

    expect(await screen.findByText(expectedSubstring)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'or fill in the details manually' })).toBeInTheDocument();
  });

  it('pkpass drop: on a 200, pre-fills manual fields and switches to the manual tab', async () => {
    mockDefaultResponse(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'KGX', destinationCrs: null, source: 'pkpass-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    dropFiles(getPkpassDropzoneRoot(), [file]);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Manual', selected: true })).toBeInTheDocument();
    });
    expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toHaveValue('LNER');
    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pkpass', expect.objectContaining({ method: 'POST' }));
    });
  });

  it('pdf drop: posts to the pdf-specific upload route, not the pkpass one', async () => {
    mockDefaultResponse(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, source: 'pdf-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'PDF' }));
    const file = new File(['fake'], 'ticket.pdf', { type: 'application/pdf' });
    dropFiles(getPdfDropzoneRoot(), [file]);

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pdf', expect.objectContaining({ method: 'POST' }));
    });
  });

  // Task 3.6.6: a single compact dropzone above the tabs accepts either
  // file kind without a tab switch first -- these two tests cover the
  // routing logic (`pickKindFor`) that tells the two kinds apart from a
  // dropped file's name, since the visible tabs and their own dedicated
  // Dropzones are unchanged and already covered above.
  describe('the combined dropzone above the tabs', () => {
    it('dropping a .pkpass file routes to the pkpass upload route, pre-fills, and lands on the manual tab', async () => {
      mockDefaultResponse(
        new Response(
          JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'KGX', destinationCrs: null, source: 'pkpass-heuristic' }),
          { status: 200 },
        ),
      );
      openForm();
      const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
      dropFiles(getCombinedDropzoneRoot(), [file]);

      await waitFor(() => {
        expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pkpass', expect.objectContaining({ method: 'POST' }));
      });
      // Same "pre-fill then land on the manual view for review" behavior as
      // the dedicated pkpass tab's own Dropzone (`applyPreview`) -- the
      // combined dropzone reuses that exact code path unmodified.
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Manual', selected: true })).toBeInTheDocument();
      });
      expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toHaveValue('LNER');
    });

    it('dropping a PDF file routes to the pdf upload route, pre-fills, and lands on the manual tab', async () => {
      mockDefaultResponse(
        new Response(
          JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, source: 'pdf-heuristic' }),
          { status: 200 },
        ),
      );
      openForm();
      const file = new File(['fake'], 'ticket.pdf', { type: 'application/pdf' });
      dropFiles(getCombinedDropzoneRoot(), [file]);

      await waitFor(() => {
        expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pdf', expect.objectContaining({ method: 'POST' }));
      });
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Manual', selected: true })).toBeInTheDocument();
      });
      expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toHaveValue('LNER');
    });

    // Unlike the success path above (which always lands back on the manual
    // tab via `applyPreview`, regardless of which kind was dropped), a
    // FAILED upload never calls `applyPreview` at all -- `setTab(kind)`
    // (called before `handleUpload`, so it isn't overwritten by success)
    // is the only thing that puts the visible tab in sync with which
    // upload actually failed.
    it('a failed .pkpass drop switches to the pkpass tab so the inline error is on the visible panel', async () => {
      mockDefaultResponse(new Response('error', { status: 500 }));
      openForm();
      const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
      dropFiles(getCombinedDropzoneRoot(), [file]);

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: '.pkpass', selected: true })).toBeInTheDocument();
      });
      expect(await screen.findByText("Couldn't read this file. Try filling in the details manually")).toBeInTheDocument();
    });
  });

  it('dropping a mismatched file type on the pkpass tab does not call fetch', async () => {
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['%PDF-1.4'], 'ticket.pdf', { type: 'application/pdf' });
    dropFiles(getPkpassDropzoneRoot(), [file]);

    // Give any (incorrect) async path a chance to run before asserting a
    // negative -- consistent with this file's existing style of asserting
    // absence via waitFor's polling rather than a bare synchronous check.
    await waitFor(() => expect(fetch).not.toHaveBeenCalled());
  });

  it('pkpass upload: on a 200, pre-fills manual fields, marks them auto-filled, and switches to the manual tab', async () => {
    mockDefaultResponse(
      new Response(
        JSON.stringify({
          operator: 'LNER',
          ticketType: null,
          originCrs: 'Kings Cross',
          destinationCrs: 'Edinburgh',
          source: 'pkpass-semantics',
        }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Manual', selected: true })).toBeInTheDocument();
    });
    expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toHaveValue('LNER');
    expect(screen.getByRole('combobox', { name: 'Origin station (optional)' })).toHaveValue('Kings Cross');
    // "Kings Cross" is not a 3-letter CRS code -- the pre-filled value
    // stays editable and is flagged for review, not silently accepted.
    // "Edinburgh" (the preview's destinationCrs) is equally not a 3-letter
    // code, so both fields render this exact description -- getByText would
    // fail on the ambiguous match, hence getAllByText/length 2 here.
    expect(screen.getAllByText('Auto-filled — please check this is a real 3-letter CRS code')).toHaveLength(2);
    expect(screen.getByRole('combobox', { name: 'Origin station (optional)' })).not.toBeDisabled();
  });

  it('editing an auto-filled field does not reset source back to manual', async () => {
    mockDefaultResponse(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'Kings Cross', destinationCrs: null, source: 'pkpass-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });
    await screen.findByRole('combobox', { name: 'Origin station (optional)' });

    // Correct the auto-filled station name into a real CRS code -- this is
    // exactly the review-before-save edit the CRS-format check exists to
    // force.
    fireEvent.change(screen.getByRole('combobox', { name: 'Origin station (optional)' }), { target: { value: 'KGX' } });

    mockDefaultResponse(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      const [, init] = findFetchCall('/api/Train/1/tickets');
      const body = JSON.parse((init as RequestInit).body as string);
      expect(body.source).toBe('pkpass-heuristic');
      expect(body.origin_crs).toBe('KGX');
    });
  });

  it('pdf upload: posts to the pdf-specific upload route, not the pkpass one', async () => {
    mockDefaultResponse(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, source: 'pdf-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'PDF' }));
    const file = new File(['fake'], 'ticket.pdf', { type: 'application/pdf' });
    fireEvent.change(getPdfFileInput(), { target: { files: [file] } });

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pdf', expect.objectContaining({ method: 'POST' }));
    });
  });

  it('a 401 during upload shows the login prompt modal, same as the final-submit 401 handling', async () => {
    mockDefaultResponse(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });
    expect(await screen.findByText('Log in to save this ticket.')).toBeInTheDocument();
  });

  // Part A of the upload-first plan: no `trackingId` prop at all -- a
  // STANDALONE ticket, uploaded/entered before a tracked train exists.
  describe('with no trackingId (standalone ticket)', () => {
    function openStandaloneForm() {
      renderWithMantine(<TicketEntryForm label="Add a ticket" />);
      fireEvent.click(screen.getByRole('button', { name: 'Add a ticket' }));
    }

    it('manual submit: POSTs to the flat /api/Train/tickets route, not a trackingId-scoped one', async () => {
      mockDefaultResponse(new Response(JSON.stringify({ ticketId: 5 }), { status: 200 }));
      openStandaloneForm();
      fireEvent.change(screen.getByRole('combobox', { name: 'Operator (optional)' }), { target: { value: 'LNER' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

      await waitFor(() => {
        expect(fetch).toHaveBeenCalledWith('/api/Train/tickets', expect.objectContaining({ method: 'POST' }));
      });
    });

    it('pkpass upload: POSTs to the flat /api/Train/tickets/pkpass route', async () => {
      mockDefaultResponse(
        new Response(
          JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'KGX', destinationCrs: null, source: 'pkpass-semantics' }),
          { status: 200 },
        ),
      );
      openStandaloneForm();
      fireEvent.click(screen.getByRole('tab', { name: '.pkpass' }));
      const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
      fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });

      await waitFor(() => {
        expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/pkpass', expect.objectContaining({ method: 'POST' }));
      });
    });

    it('on a successful save, shows the "find or track the train" next step instead of just closing', async () => {
      mockDefaultResponse(new Response(JSON.stringify({ ticketId: 5 }), { status: 200 }));
      openStandaloneForm();
      fireEvent.change(screen.getByRole('combobox', { name: 'Origin station (optional)' }), { target: { value: 'kgx' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

      const link = await screen.findByRole('link', { name: 'Find or track the train this ticket is for' });
      // The extracted/typed origin (uppercased) and the new ticket's id are
      // both carried forward, so `/track`'s own form can pre-fill the
      // origin and attach this ticket automatically once a pin is created.
      expect(link).toHaveAttribute('href', '/track?origin=KGX&ticketId=5');
      // The manual-entry form itself is gone -- replaced by this next step.
      expect(screen.queryByRole('combobox', { name: 'Operator (optional)' })).not.toBeInTheDocument();
    });

    it('the "find or track" link omits origin when none was entered', async () => {
      mockDefaultResponse(new Response(JSON.stringify({ ticketId: 6 }), { status: 200 }));
      openStandaloneForm();
      fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

      const link = await screen.findByRole('link', { name: 'Find or track the train this ticket is for' });
      expect(link).toHaveAttribute('href', '/track?ticketId=6');
    });
  });
});
