import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RenameTrainButton } from './RenameTrainButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RenameTrainButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call the API until Save is clicked', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('Save is disabled when the input is empty', async () => {
    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const save = await screen.findByRole('button', { name: 'Save' });
    expect(save).toBeDisabled();
  });

  it('POSTs the trimmed name and refreshes on success', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ customName: 'My commute' }), { status: 200 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: '  My commute  ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/42/name', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: 'My commute' }),
      });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('Clear is only shown when a custom name is currently set, and posts null', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ customName: null }), { status: 200 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName="My commute" defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Clear' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/42/name', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: null }),
      });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('does not render a Clear button when there is no custom name yet', async () => {
    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    await screen.findByRole('button', { name: 'Save' });
    expect(screen.queryByRole('button', { name: 'Clear' })).not.toBeInTheDocument();
  });

  it('shows the backend error text on a 400', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response('That name is too long — custom names can be at most 100 characters.', { status: 400 }),
    );

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: 'x'.repeat(101) } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(
        screen.getByText('That name is too long — custom names can be at most 100 characters.'),
      ).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  // Bug: `router.refresh()` preserves client component state (it re-runs
  // Server Components, it does not remount this one), so a `saving` flag
  // left `true` when it fired stayed `true` forever -- `handleOpen` resets
  // `value`/`error` but never touched `saving`, so every future open of
  // this modal rendered "Save" permanently disabled/spinning. The fix
  // resets `saving` in the same synchronous continuation as `close()`/
  // `router.refresh()` rather than only in the `catch` branch.
  it('Save is usable again on a later open, not stuck disabled from the previous save', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ customName: 'My commute' }), { status: 200 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    let input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: 'My commute' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());

    // Reopen (the prop-driven `customName` hasn't changed in this test, since
    // nothing re-renders the component with a new prop -- only the real app's
    // `router.refresh()` would do that -- so the field starts empty again).
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: 'A new name' } });
    expect(screen.getByRole('button', { name: 'Save' })).not.toBeDisabled();
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: 'My commute' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await screen.findByRole('link', { name: 'Log in to rename this tracked train' });
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
