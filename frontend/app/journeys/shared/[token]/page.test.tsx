import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import SharedJourneyPage from './page';
import { getJourney, getJourneyByShareToken, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getJourney: vi.fn(),
    getJourneyByShareToken: vi.fn(),
  };
});

// `redirect()` mocked the same way `app/train/by-id/[trackingId]/page.test.tsx`
// mocks it -- its real behaviour (throwing a Next.js-internal,
// digest-carrying error caught by framework machinery above the page)
// doesn't work outside a real App Router tree, so tests assert on the mock
// having been called with the right URL instead of on any real navigation
// happening.
const redirectMock = vi.fn((url: string) => {
  throw new Error(`NEXT_REDIRECT:${url}`);
});

vi.mock('next/navigation', () => ({
  redirect: (url: string) => redirectMock(url),
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/journeys/shared/tok123',
  useSearchParams: () => new URLSearchParams(''),
}));

// `getSiteOrigin()` (lib/siteOrigin.ts) reads `next/headers` when
// `NEXT_PUBLIC_SITE_URL` isn't set -- there is no Next request context in a
// unit test. Same stub shape `app/journeys/[id]/page.test.tsx` uses for the
// same reason.
vi.mock('next/headers', () => ({
  headers: async () => ({ get: () => null }),
}));

function matchedLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'YRK',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

// Matches `getJourneyByShareToken`'s own real, server-side guarantee
// (Task 2): `isOwner` is always `false` and `shareLink` is always `null` on
// the object it returns.
function sharedJourney(overrides: Partial<JourneyDetail> = {}): JourneyDetail {
  return {
    id: 167,
    customName: null,
    createdAt: '2026-09-22T00:00:00Z',
    legs: [matchedLeg()],
    isOwner: false,
    shareLink: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.mocked(getJourney).mockReset();
  vi.mocked(getJourneyByShareToken).mockReset();
  redirectMock.mockClear();
});

async function renderPage(token = 'tok123') {
  return renderWithMantine(await SharedJourneyPage({ params: Promise.resolve({ token }) }));
}

describe('SharedJourneyPage', () => {
  it('renders "Link not found" on an invalid/expired/revoked token, never calling getJourney', async () => {
    vi.mocked(getJourneyByShareToken).mockRejectedValue(new ApiNotFoundError('404'));

    await renderPage('bad-token');

    expect(screen.getByRole('heading', { name: 'Link not found' })).toBeInTheDocument();
    expect(screen.getByText(/This share link is invalid or has been revoked/)).toBeInTheDocument();
    expect(getJourney).not.toHaveBeenCalled();
  });

  it('redirects to the canonical journey page when the viewer is already authorized', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(sharedJourney({ id: 167 }));
    vi.mocked(getJourney).mockResolvedValue(sharedJourney({ id: 167, isOwner: true }));

    await expect(renderPage()).rejects.toThrow('NEXT_REDIRECT:/journeys/167');

    expect(redirectMock).toHaveBeenCalledWith('/journeys/167');
    // The token-scoped view must never have rendered any leg content on the
    // way to the redirect -- this is a redirect, not a second, degraded
    // rendering of the same data.
    expect(screen.queryByText(/Change train/)).not.toBeInTheDocument();
  });

  it('renders the token-scoped view when the viewer is not logged in at all (ApiUnauthorizedError)', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(sharedJourney());
    vi.mocked(getJourney).mockRejectedValue(new ApiUnauthorizedError('401'));

    await renderPage();

    expect(redirectMock).not.toHaveBeenCalled();
    expect(screen.getByText(/You're viewing this journey via a shared link/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1 })).toBeInTheDocument();
  });

  it('renders the token-scoped view when the viewer is logged in but not authorized (ApiNotFoundError)', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(sharedJourney());
    vi.mocked(getJourney).mockRejectedValue(new ApiNotFoundError('404'));

    await renderPage();

    expect(redirectMock).not.toHaveBeenCalled();
    expect(screen.getByText(/You're viewing this journey via a shared link/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1 })).toBeInTheDocument();
  });

  it('propagates an unexpected error from the authorized-probe call', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(sharedJourney());
    vi.mocked(getJourney).mockRejectedValue(new Error('boom'));

    await expect(renderPage()).rejects.toThrow('boom');
    expect(redirectMock).not.toHaveBeenCalled();
  });

  it('shows no owner-only actions in the token-scoped view (isOwner: false)', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(sharedJourney());
    vi.mocked(getJourney).mockRejectedValue(new ApiUnauthorizedError('401'));

    await renderPage();

    expect(screen.queryByRole('button', { name: 'Share with a group' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Get shareable link' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Manage shared link' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a leg' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save as template' })).not.toBeInTheDocument();
  });
});
