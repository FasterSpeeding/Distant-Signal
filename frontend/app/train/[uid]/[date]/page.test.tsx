import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackedTrainByUidPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import type { TrackedTrainState } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTrackedTrainByUidAndDate: vi.fn() };
});
// Mocked so a real Next.js `notFound()` throw (its actual behaviour) doesn't
// require an app-router tree to render -- same pattern as
// app/lines/[id]/page.test.tsx. `useRouter()` is also mocked -- DeleteTrainButton
// (rendered on the success path) calls it from next/navigation, which throws
// outside a real Next.js App Router tree -- same workaround
// app/lines/[id]/page.test.tsx uses for DeleteLineButton.
const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
// usePathname()/useSearchParams() stubbed for the same reason as
// AuthStatus.test.tsx/TicketPanel.test.tsx -- the login-error branch now
// renders LoginLink (Task 1).
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/train/W12345/2026-08-31',
  useSearchParams: () => new URLSearchParams(''),
}));
// `TicketPanel` is itself an async Server Component (it does its own
// `getSession()`/ownership-probe fetches) -- React's plain DOM test
// renderer, used via `renderWithMantine` here, can't render an async
// function component at all (that's an RSC-only capability), so it has to
// be stubbed out for a success-path test that renders the whole page.
// Only this file's success-path test below cares about `DeleteTrainButton`
// being present, not `TicketPanel`'s own rendering, which is already
// covered by `TicketPanel.test.tsx`.
vi.mock('@/components/TicketPanel', () => ({
  TicketPanel: () => null,
}));

function trackedTrainState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 42,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    trainId: '1A23',
    status: 'en_route',
    lastReportedLocation: 'Woking',
    lastEventType: 'DEPARTURE',
    delayMinutes: 0,
    nextCallingPoint: 'Basingstoke',
    etaNext: null,
    etaSource: null,
    ...overrides,
    customName: overrides.customName ?? null,
  };
}

async function renderPage(uid = 'W12345', date = '2026-08-31') {
  const element = await TrackedTrainByUidPage({ params: Promise.resolve({ uid, date }) });
  return renderWithMantine(element);
}

describe('TrackedTrainByUidPage error handling', () => {
  beforeEach(() => {
    notFoundMock.mockClear();
  });

  it('shows a login prompt linking to /api/auth/login on ApiUnauthorizedError', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockRejectedValue(new ApiUnauthorizedError('unauthorized'));
    await renderPage();
    const link = screen.getByRole('link', { name: 'Log in to view this tracked train' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrain%2FW12345%2F2026-08-31');
    expect(notFoundMock).not.toHaveBeenCalled();
  });

  it('still calls notFound() on ApiNotFoundError, unswallowed by the new branch', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(renderPage()).rejects.toThrow('NEXT_NOT_FOUND');
    expect(notFoundMock).toHaveBeenCalled();
  });

  it('still propagates a bare Error uncaught', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockRejectedValue(new Error('boom'));
    await expect(renderPage()).rejects.toThrow('boom');
    expect(notFoundMock).not.toHaveBeenCalled();
  });
});

describe('TrackedTrainByUidPage success path', () => {
  it('renders a Delete button once the tracked train state loads', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockResolvedValue(trackedTrainState());
    await renderPage();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });
});
