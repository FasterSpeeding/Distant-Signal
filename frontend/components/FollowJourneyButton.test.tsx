import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { FollowJourneyButton } from './FollowJourneyButton';

const refreshMock = vi.fn();
const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock, push: pushMock }),
  usePathname: () => '/journeys/7',
  useSearchParams: () => new URLSearchParams(''),
}));

function lastCall(): [string, RequestInit | undefined] {
  const call = vi.mocked(fetch).mock.calls.at(-1);
  return [call?.[0] as string, call?.[1]];
}

describe('FollowJourneyButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('follows through the given group and refreshes', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('{}', { status: 200 }));
    renderWithMantine(<FollowJourneyButton journeyId={7} following={false} groupId="grp-1" />);

    fireEvent.click(screen.getByRole('button', { name: 'Follow this journey' }));

    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    const [url, init] = lastCall();
    expect(url).toBe('/api/Journeys/7/follow');
    expect(init?.method).toBe('POST');
    expect(JSON.parse(init?.body as string)).toEqual({ groupId: 'grp-1' });
  });

  it('follows with a share token in the body, never the URL, then opens the journey', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('{}', { status: 200 }));
    renderWithMantine(
      <FollowJourneyButton journeyId={7} following={false} shareToken="tok_123" afterFollowHref="/journeys/7" />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Follow this journey' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys/7'));
    const [url, init] = lastCall();
    expect(url).not.toContain('tok_123');
    expect(JSON.parse(init?.body as string)).toEqual({ shareToken: 'tok_123' });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('sends no body when no share is named', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('{}', { status: 200 }));
    renderWithMantine(<FollowJourneyButton journeyId={7} following={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Follow this journey' }));

    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(lastCall()[1]?.body).toBeUndefined();
  });

  it('unfollows with a DELETE, and leaves the page when told to', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 204 }));
    renderWithMantine(<FollowJourneyButton journeyId={7} following groupId="grp-1" afterUnfollowHref="/journeys" />);

    fireEvent.click(screen.getByRole('button', { name: 'Unfollow' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys'));
    const [url, init] = lastCall();
    expect(url).toBe('/api/Journeys/7/follow');
    expect(init?.method).toBe('DELETE');
    expect(init?.body).toBeUndefined();
  });

  it('asks a logged-out visitor to log in', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 401 }));
    renderWithMantine(<FollowJourneyButton journeyId={7} following={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Follow this journey' }));

    expect(await screen.findByText('Log in to follow this journey')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it("shows the api's own refusal, such as for the caller's own journey", async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('this is your own journey', { status: 409 }));
    renderWithMantine(<FollowJourneyButton journeyId={7} following={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Follow this journey' }));

    expect(await screen.findByText('This is your own journey.')).toBeInTheDocument();
  });
});
