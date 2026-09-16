import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddCustomLineToGroupButton } from './AddCustomLineToGroupButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

/** `/api/lines` returns catalogue, TfL and (caller-scoped) custom entries
 * all in one list -- the picker has to narrow that to custom. */
function linesResponse() {
  return new Response(
    JSON.stringify([
      { id: 'wcml', name: 'West Coast Main Line', category: 'main-line', operators: ['VT'], source: 'catalogue' },
      { id: 'tfl-victoria', name: 'Victoria (TfL)', category: 'tube', operators: ['TFL'], source: 'tfl' },
      { id: 'custom-my-commute', name: 'My Commute', category: 'custom', operators: ['SW'], source: 'custom' },
      { id: 'custom-weekend', name: 'Weekend Run', category: 'custom', operators: ['SW'], source: 'custom' },
    ]),
    { status: 200 },
  );
}

describe('AddCustomLineToGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('offers only the caller\'s own custom lines, never catalogue or TfL ones', async () => {
    // `/public/lines` is already caller-scoped for custom entries, so
    // "source === custom" is exactly "lines I own". A catalogue or TfL id
    // offered here would only ever 404 server-side (grant_custom_line
    // checks ownership against `custom_lines`), so it must never appear.
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(linesResponse());

    renderWithMantine(<AddCustomLineToGroupButton groupId="grp-1" excludeLineIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Share one of my custom lines' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/lines'));
    const [select] = await screen.findAllByLabelText('Custom line');
    fireEvent.click(select);

    expect(await screen.findByText('My Commute')).toBeInTheDocument();
    expect(screen.getByText('Weekend Run')).toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
    expect(screen.queryByText('Victoria (TfL)')).not.toBeInTheDocument();
  });

  it('excludes lines already shared into this group', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(linesResponse());

    renderWithMantine(
      <AddCustomLineToGroupButton groupId="grp-1" excludeLineIds={['custom-weekend']} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Share one of my custom lines' }));

    const [select] = await screen.findAllByLabelText('Custom line');
    fireEvent.click(select);

    expect(await screen.findByText('My Commute')).toBeInTheDocument();
    expect(screen.queryByText('Weekend Run')).not.toBeInTheDocument();
  });

  it('POSTs the chosen lineId to the group grant route and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/lines') return Promise.resolve(linesResponse());
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<AddCustomLineToGroupButton groupId="grp-1" excludeLineIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Share one of my custom lines' }));
    const [select] = await screen.findAllByLabelText('Custom line');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('My Commute'));
    fireEvent.click(screen.getByRole('button', { name: 'Share with group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1/lines/custom',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ lineId: 'custom-my-commute' }),
        }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('surfaces the backend\'s own refusal message rather than swallowing it', async () => {
    // The server-side ownership check is the real boundary; if it refuses
    // (404 "custom line not found"), the user must see why rather than a
    // silently-closed modal.
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/lines') return Promise.resolve(linesResponse());
      return Promise.resolve(new Response('custom line not found', { status: 404 }));
    });

    renderWithMantine(<AddCustomLineToGroupButton groupId="grp-1" excludeLineIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Share one of my custom lines' }));
    const [select] = await screen.findAllByLabelText('Custom line');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('My Commute'));
    fireEvent.click(screen.getByRole('button', { name: 'Share with group' }));

    expect(await screen.findByText('custom line not found')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
