import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import SharedJourneyPage, { generateMetadata } from './page';
import { getJourney, getJourneyByShareToken, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import type { JourneyDetail, JourneyLegDetail, TrackedTrainState } from '@/lib/types';

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

// Same fixture shape as `components/JourneyStatusBadge.test.tsx`'s own
// `baseTrackedTrainState` -- kept in sync deliberately so both test files
// agree on what a "real" tracked-train state looks like.
function baseTrackedTrainState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    sharedGroupCount: 0,
    customName: null,
    ...overrides,
  };
}

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

  // Finding 3 of the 2026-09-24 security review: a malformed token (a
  // `../` segment, an embedded `?`/`#`) used to reach
  // `getJourneyByShareToken` completely unvalidated. Treated the same as
  // an unknown/expired token -- the same "Link not found" copy -- but
  // without ever calling the API at all.
  it('renders "Link not found" for a malformed token, without ever calling getJourneyByShareToken', async () => {
    await renderPage('../evil');

    expect(screen.getByRole('heading', { name: 'Link not found' })).toBeInTheDocument();
    expect(getJourneyByShareToken).not.toHaveBeenCalled();
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

describe('generateMetadata', () => {
  it('titles the page with the route from first leg origin to last leg destination', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({
        legs: [
          matchedLeg({ originName: 'London Kings Cross', destinationName: 'Peterborough' }),
          matchedLeg({ id: 2, originName: 'Peterborough', destinationName: 'York' }),
        ],
      }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.title).toBe('London Kings Cross to York — Distant Signal');
    expect(metadata.openGraph?.title).toBe('London Kings Cross to York — Distant Signal');
    expect(metadata.twitter).toMatchObject({ card: 'summary', title: 'London Kings Cross to York — Distant Signal' });
  });

  it('falls back to CRS codes for the title when no station names are resolved', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({ legs: [matchedLeg({ originCrs: 'KGX', originName: null, destinationCrs: 'YRK', destinationName: null })] }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.title).toBe('KGX to YRK — Distant Signal');
  });

  it('falls back to a generic title when the leg has no origin/destination at all (unmatched, no window)', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({
        legs: [matchedLeg({ originCrs: null, originName: null, destinationCrs: null, destinationName: null, trackedTrainState: null })],
      }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.title).toBe('Shared journey — Distant Signal');
  });

  it('describes an on-time journey with its date', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({
        legs: [matchedLeg({ serviceDate: '2026-09-22', trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 0 }) })],
      }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.description).toBe('A journey on 22 Sept 2026, on time.');
    expect(metadata.openGraph?.description).toBe('A journey on 22 Sept 2026, on time.');
  });

  it('describes a delayed journey without quoting the exact delay figure', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({
        legs: [
          matchedLeg({ serviceDate: '2026-09-22', trackedTrainState: baseTrackedTrainState({ delayMinutes: 47 }) }),
        ],
      }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.description).toBe('A journey on 22 Sept 2026, running delayed.');
    expect(metadata.description).not.toMatch(/47/);
    expect(metadata.description).not.toMatch(/m late/);
  });

  it('describes a cancelled journey', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({
        legs: [matchedLeg({ trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }) })],
      }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.description).toBe('A journey on 22 Sept 2026, cancelled.');
  });

  it('describes an unmatched leg as still needing a train picked', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({ legs: [matchedLeg({ matchMode: 'unmatched', trackedTrainState: null })] }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'tok123' }) });

    expect(metadata.description).toBe('A journey on 22 Sept 2026, still needs a train picked.');
  });

  // Never leaks the bearer token (or the journey's own internal numeric id)
  // into publicly-crawlable metadata -- the token IS the secret that grants
  // access to this journey; the id is unused but excluded on the same
  // "no reason to expose it" principle. Nor does it leak the owner-chosen
  // free-text `customName` -- title/description are built from the route
  // and a coarse status only.
  it('never includes the share token, the journey id, or the owner-chosen custom name in the metadata', async () => {
    vi.mocked(getJourneyByShareToken).mockResolvedValue(
      sharedJourney({
        id: 167,
        customName: "Mum's birthday trip",
        legs: [matchedLeg({ originName: 'London Kings Cross', destinationName: 'York' })],
      }),
    );

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'super-secret-tok-abc123' }) });

    const rendered = JSON.stringify(metadata);
    expect(rendered).not.toContain('super-secret-tok-abc123');
    expect(rendered).not.toContain('167');
    expect(rendered).not.toContain("Mum's birthday trip");
  });

  // Regression-shaped, matching `app/groups/join/[token]/page.test.tsx`'s
  // own equivalent case: this used to have no `generateMetadata` at all,
  // so a bare `notFound()` was never a live risk here -- but the same
  // reasoning applies now that one exists: `generateMetadata` runs
  // independently of the page component and must not 404 the WHOLE route
  // for an expired/revoked link, only degrade its own metadata.
  it('falls back to site-wide metadata on ApiNotFoundError, without 404ing the route', async () => {
    vi.mocked(getJourneyByShareToken).mockRejectedValue(new ApiNotFoundError('not found'));

    const metadata = await generateMetadata({ params: Promise.resolve({ token: 'bad-token' }) });

    expect(metadata).toEqual({});
  });

  it('falls back to site-wide metadata for a malformed token, without ever calling getJourneyByShareToken', async () => {
    vi.mocked(getJourneyByShareToken).mockClear();

    const metadata = await generateMetadata({ params: Promise.resolve({ token: '../evil' }) });

    expect(metadata).toEqual({});
    expect(getJourneyByShareToken).not.toHaveBeenCalled();
  });

  it('propagates an unexpected error', async () => {
    vi.mocked(getJourneyByShareToken).mockRejectedValue(new Error('boom'));

    await expect(generateMetadata({ params: Promise.resolve({ token: 'tok123' }) })).rejects.toThrow('boom');
  });
});
