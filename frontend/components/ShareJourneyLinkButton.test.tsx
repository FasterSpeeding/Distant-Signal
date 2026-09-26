import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ShareJourneyLinkButton } from './ShareJourneyLinkButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

// Same reasoning as `GroupInviteLinkCard.test.tsx`'s own `ORIGIN` constant:
// a value that could never equal jsdom's own default origin, so every
// assertion below makes it obvious the rendered URL came from the prop,
// not from `window.location`.
const ORIGIN = 'https://distant-signal.example';

describe('ShareJourneyLinkButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders "Get shareable link" when there is no active link, and the modal offers "Create link" only', async () => {
    renderWithMantine(<ShareJourneyLinkButton journeyId={167} shareLink={null} origin={ORIGIN} />);
    expect(screen.getByRole('button', { name: 'Get shareable link' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Manage shared link' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Get shareable link' }));

    // Mantine's `Modal` renders into a portal via a transition, so its
    // content is not present synchronously right after the click -- same
    // reason `ShareJourneyButton.test.tsx` awaits its own modal's first
    // control with `findByRole` rather than `getByRole`.
    expect(await screen.findByRole('button', { name: 'Create link' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Regenerate' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Revoke' })).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Share link')).not.toBeInTheDocument();
    expect(screen.getByText('No active share link.')).toBeInTheDocument();
  });

  it('renders "Manage shared link" when a link exists, and the modal shows the built URL, Regenerate, and Revoke', async () => {
    renderWithMantine(
      <ShareJourneyLinkButton
        journeyId={167}
        shareLink={{ token: 'tok123', expiresAt: null }}
        origin={ORIGIN}
      />,
    );
    expect(screen.getByRole('button', { name: 'Manage shared link' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Get shareable link' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));

    expect(await screen.findByDisplayValue(`${ORIGIN}/journeys/shared/tok123`)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Regenerate' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Revoke' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create link' })).not.toBeInTheDocument();
  });

  it('states that the link works until it expires or is revoked', async () => {
    renderWithMantine(
      <ShareJourneyLinkButton
        journeyId={167}
        shareLink={{ token: 'tok123', expiresAt: null }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));
    expect(
      await screen.findByText(
        /until it expires or you\s+revoke it\./,
      ),
    ).toBeInTheDocument();
  });

  it('Create link POSTs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ token: 'new', expiresAt: null }), { status: 200 }),
    );

    renderWithMantine(<ShareJourneyLinkButton journeyId={167} shareLink={null} origin={ORIGIN} />);
    fireEvent.click(screen.getByRole('button', { name: 'Get shareable link' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create link' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/share-link', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());

    // Regression for the review finding: `busy` must be reset on the
    // success path too, not only on failure -- otherwise `loading={busy}`
    // (which Mantine's `Button` treats as `disabled`) leaves this button
    // permanently unclickable once a link has been created, since
    // `router.refresh()` reconciles this client component in place rather
    // than remounting it.
    // Tokens are hashed at rest (L14): the ONLY time a copyable URL exists
    // is right after this POST, so the component shows it from the
    // response itself (and the button flips to Regenerate).
    await waitFor(() => expect(screen.getByRole('button', { name: 'Regenerate' })).toBeEnabled());
    expect(screen.getByRole('textbox', { name: 'Share link' })).toHaveValue(`${ORIGIN}/journeys/shared/new`);
  });

  it('shows the expiry and Extend keeps the same link, updating the expiry', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ token: null, expiresAt: '2026-11-20T12:00:00Z' }), { status: 200 }),
    );
    renderWithMantine(
      <ShareJourneyLinkButton
        journeyId={167}
        shareLink={{ token: 'tok123', expiresAt: '2026-10-20T12:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));
    expect(await screen.findByText(/^Expires /)).toBeInTheDocument();
    const before = screen.getByText(/^Expires /).textContent;
    fireEvent.click(screen.getByRole('button', { name: 'Extend 30 days' }));
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/share-link/extend', { method: 'POST' });
    });
    await waitFor(() => expect(screen.getByText(/^Expires /).textContent).not.toBe(before));
    expect(screen.getByDisplayValue(`${ORIGIN}/journeys/shared/tok123`)).toBeInTheDocument();
  });

  it('an existing link whose token is not returned (hashed at rest) is described, not shown', async () => {
    renderWithMantine(
      <ShareJourneyLinkButton journeyId={167} shareLink={{ token: null, expiresAt: null }} origin={ORIGIN} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));
    expect(await screen.findByText(/can.t be shown again/)).toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: 'Share link' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Revoke' })).toBeInTheDocument();
  });

  it('Regenerate POSTs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ token: 'new', expiresAt: null }), { status: 200 }),
    );

    renderWithMantine(
      <ShareJourneyLinkButton
        journeyId={167}
        shareLink={{ token: 'tok123', expiresAt: null }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Regenerate' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/share-link', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());

    // Same regression as the "Create link" test above, but for Regenerate:
    // a successful regenerate must not leave Regenerate/Revoke stuck
    // disabled forever.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Regenerate' })).toBeEnabled());
    expect(screen.getByRole('button', { name: 'Revoke' })).toBeEnabled();
  });

  it('Revoke DELETEs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(
      <ShareJourneyLinkButton
        journeyId={167}
        shareLink={{ token: 'tok123', expiresAt: null }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Revoke' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/share-link', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());

    // Same regression as the "Create link" test above, but for Revoke:
    // a successful revoke must not leave Regenerate/Revoke stuck disabled
    // forever.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Revoke' })).toBeEnabled());
    expect(screen.getByRole('button', { name: 'Regenerate' })).toBeEnabled();
  });

  it('a 401 on Create/Regenerate shows a login prompt, not the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<ShareJourneyLinkButton journeyId={167} shareLink={null} origin={ORIGIN} />);
    fireEvent.click(screen.getByRole('button', { name: 'Get shareable link' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create link' }));

    expect(
      await screen.findByRole('link', { name: "Log in to manage this journey's share link" }),
    ).toBeInTheDocument();
    expect(screen.queryByText('Could not create a share link.')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a 401 on Revoke shows a login prompt, not the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(
      <ShareJourneyLinkButton
        journeyId={167}
        shareLink={{ token: 'tok123', expiresAt: null }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Manage shared link' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Revoke' }));

    expect(
      await screen.findByRole('link', { name: "Log in to manage this journey's share link" }),
    ).toBeInTheDocument();
    expect(screen.queryByText('Could not revoke the share link.')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a non-401 failure still shows the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('boom', { status: 500 }));

    renderWithMantine(<ShareJourneyLinkButton journeyId={167} shareLink={null} origin={ORIGIN} />);
    fireEvent.click(screen.getByRole('button', { name: 'Get shareable link' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create link' }));

    expect(await screen.findByText('Could not create a share link.')).toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: "Log in to manage this journey's share link" }),
    ).not.toBeInTheDocument();
  });
});
