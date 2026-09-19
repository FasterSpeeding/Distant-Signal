import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GroupInviteLinkCard } from './GroupInviteLinkCard';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

// Fixed rather than reading `window.location.origin`: the whole point of
// review §2.11's fix is that this component no longer reads `window` at
// all -- `origin` is now an ordinary prop the caller (a Server Component)
// resolves via `lib/siteOrigin.ts`. Using a value that could never equal
// jsdom's own default origin also makes it obvious, in every assertion
// below, that the rendered URL came from the prop.
const ORIGIN = 'https://distant-signal.example';

describe('GroupInviteLinkCard', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    // @ts-expect-error -- undo the share test's navigator.clipboard stub,
    // the same cleanup ShareButton.test.tsx does for the same property.
    delete navigator.clipboard;
  });

  it('shows "No active invite link" when there is none', () => {
    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} origin={ORIGIN} />);
    expect(screen.getByText('No active invite link.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Revoke' })).not.toBeInTheDocument();
  });

  it('renders the full join URL built from the token', () => {
    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    expect(screen.getByDisplayValue(`${ORIGIN}/groups/join/tok123`)).toBeInTheDocument();
  });

  /** Review §2.11: `origin` is a plain prop, resolved server-side before
   * this component ever renders -- so the absolute URL is present and
   * copyable/shareable on the FIRST render, with no mount-effect window
   * where it reads as a bare, uncopyable path (or Share silently no-ops
   * against `origin === ''`). Asserting with a synchronous `getBy*`
   * (rather than `findBy*`/`waitFor`) is the actual proof: this would fail
   * immediately, not just eventually, if the URL were still built from a
   * `useEffect`. */
  it('renders the absolute join URL synchronously, with no effect to wait for', () => {
    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    const input = screen.getByDisplayValue(`${ORIGIN}/groups/join/tok123`);
    expect(input).toBeInTheDocument();
  });

  it('shares (or copies) the absolute URL immediately, with no origin-readiness gate', async () => {
    // jsdom implements neither `navigator.share` nor `navigator.clipboard`
    // -- same `Object.defineProperty` stubbing shape ShareButton.test.tsx
    // uses for the same reason (these live on `navigator`, not
    // `globalThis`, so `vi.stubGlobal` doesn't reach them).
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      writable: true,
      configurable: true,
    });

    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Share invite link' }));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith(`${ORIGIN}/groups/join/tok123`));
  });

  // Review §2.10: Mantine's default `md` `ActionIcon` (28px) is under the
  // review's recommended sizing for this icon-button set. Sized to 36px to
  // match the adjacent `TextInput`'s own height rather than the 44px
  // primary-action floor `PinToggle`/`ShareButton` get -- see this
  // component's own comment for the rationale. See `PinToggle.test.tsx`'s
  // identical assertion shape for why the expected value is a `calc()`
  // expression rather than a plain pixel string.
  it('sizes the share button to 36px, above the 24px touch-target floor', () => {
    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    expect(screen.getByRole('button', { name: 'Share invite link' })).toHaveStyle({
      '--ai-size': 'calc(2.25rem * var(--mantine-scale))',
    });
  });

  it('Regenerate POSTs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ token: 'new', expiresAt: '2026-09-19T00:00:00Z' }), { status: 200 }));

    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('Revoke DELETEs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('a 401 on Regenerate shows a login prompt, not the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} origin={ORIGIN} />);
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    expect(await screen.findByRole('link', { name: 'Log in to manage this invite link' })).toBeInTheDocument();
    expect(screen.queryByText('Could not create a new invite link.')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a 401 on Revoke shows a login prompt, not the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));

    expect(await screen.findByRole('link', { name: 'Log in to manage this invite link' })).toBeInTheDocument();
    expect(screen.queryByText('Could not revoke the invite link.')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a non-401 failure still shows the generic error', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('boom', { status: 500 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} origin={ORIGIN} />);
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    expect(await screen.findByText('Could not create a new invite link.')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in to manage this invite link' })).not.toBeInTheDocument();
  });
});
