import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TicketEntryForm } from './TicketEntryForm';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
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

  it('starts collapsed, showing only the entry-point button', () => {
    renderWithMantine(<TicketEntryForm trackingId={1} label="Add a ticket for this journey" />);
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Operator')).not.toBeInTheDocument();
  });

  it('expands into the manual-entry tab by default when opened', () => {
    openForm();
    expect(screen.getByLabelText('Operator')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Manual entry', selected: true })).toBeInTheDocument();
  });

  it('manual submit: on success, saves, collapses, and refreshes the page', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
    openForm();
    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(
        '/api/Train/1/tickets',
        expect.objectContaining({ method: 'POST' }),
      );
    });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({ operator: 'LNER', source: 'manual' });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
  });

  it('manual submit: on a 401, shows the login prompt and preserves typed fields', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to save this ticket' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login');
    expect(screen.getByLabelText('Operator')).toHaveValue('LNER');
  });

  it('manual submit: on a 400, shows the backend message inline', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('origin_crs must be a 3-letter CRS code', { status: 400 }));
    openForm();
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));
    expect(await screen.findByText('origin_crs must be a 3-letter CRS code')).toBeInTheDocument();
  });

  it.each([
    [400, "That doesn't look like a valid upload — try again or fill in the form manually"],
    [422, 'could not read this as a train .pkpass: not a zip file'],
    [504, 'That file took too long to read — try a smaller or simpler PDF, or fill in the details manually'],
    [413, 'That file is too large (8 MB limit). Try filling in the details manually'],
    [500, "Couldn't read this file. Try filling in the details manually"],
  ])('pkpass upload: a %i response shows the mapped inline message', async (status, expectedSubstring) => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(status === 422 ? 'could not read this as a train .pkpass: not a zip file' : 'error', { status }),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });

    expect(await screen.findByText(expectedSubstring)).toBeInTheDocument();
    // The manual form must stay reachable regardless of why the upload
    // failed.
    expect(screen.getByRole('button', { name: 'or fill in the details manually' })).toBeInTheDocument();
  });

  it('pkpass upload: on a 200, pre-fills manual fields, marks them auto-filled, and switches to the manual tab', async () => {
    vi.mocked(fetch).mockResolvedValue(
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
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Manual entry', selected: true })).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Operator')).toHaveValue('LNER');
    expect(screen.getByLabelText('Origin CRS code')).toHaveValue('Kings Cross');
    // "Kings Cross" is not a 3-letter CRS code -- the pre-filled value
    // stays editable and is flagged for review, not silently accepted.
    // "Edinburgh" (the preview's destinationCrs) is equally not a 3-letter
    // code, so both fields render this exact description -- getByText would
    // fail on the ambiguous match, hence getAllByText/length 2 here.
    expect(screen.getAllByText('Auto-filled — please check this is a real 3-letter CRS code')).toHaveLength(2);
    expect(screen.getByLabelText('Origin CRS code')).not.toBeDisabled();
  });

  it('editing an auto-filled field does not reset source back to manual', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'Kings Cross', destinationCrs: null, source: 'pkpass-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });
    await screen.findByLabelText('Origin CRS code');

    // Correct the auto-filled station name into a real CRS code -- this is
    // exactly the review-before-save edit the CRS-format check exists to
    // force.
    fireEvent.change(screen.getByLabelText('Origin CRS code'), { target: { value: 'KGX' } });

    vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      const [, init] = vi.mocked(fetch).mock.calls.at(-1)!;
      const body = JSON.parse((init as RequestInit).body as string);
      expect(body.source).toBe('pkpass-heuristic');
      expect(body.origin_crs).toBe('KGX');
    });
  });

  it('pdf upload: posts to the pdf-specific upload route, not the pkpass one', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, source: 'pdf-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload PDF e-ticket' }));
    const file = new File(['fake'], 'ticket.pdf', { type: 'application/pdf' });
    fireEvent.change(getPdfFileInput(), { target: { files: [file] } });

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/1/tickets/pdf', expect.objectContaining({ method: 'POST' }));
    });
  });

  it('a 401 during upload shows the login prompt, same as the final-submit 401 handling', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });
    expect(await screen.findByRole('link', { name: 'Log in to save this ticket' })).toBeInTheDocument();
  });
});
