import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackedTrainByIdPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import type { TrackedTrainState } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTrackedTrainById: vi.fn() };
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
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  useRouter: () => ({ push: vi.fn() }),
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
    resolutionStatus: 'pending',
    trainUid: null,
    trainId: null,
    status: null,
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    ...overrides,
  };
}

async function renderPage(trackingId = '42') {
  const element = await TrackedTrainByIdPage({ params: Promise.resolve({ trackingId }) });
  return renderWithMantine(element);
}

describe('TrackedTrainByIdPage error handling', () => {
  beforeEach(() => {
    notFoundMock.mockClear();
  });

  it('shows a login prompt linking to /api/auth/login on ApiUnauthorizedError', async () => {
    vi.mocked(api.getTrackedTrainById).mockRejectedValue(new ApiUnauthorizedError('unauthorized'));
    await renderPage('42');
    const link = screen.getByRole('link', { name: 'Log in to view this tracked train' });
    expect(link).toHaveAttribute('href', '/api/auth/login');
    expect(notFoundMock).not.toHaveBeenCalled();
  });

  it('still calls notFound() on ApiNotFoundError, unswallowed by the new branch', async () => {
    vi.mocked(api.getTrackedTrainById).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(renderPage('42')).rejects.toThrow('NEXT_NOT_FOUND');
    expect(notFoundMock).toHaveBeenCalled();
  });

  it('still propagates a bare Error uncaught', async () => {
    vi.mocked(api.getTrackedTrainById).mockRejectedValue(new Error('boom'));
    await expect(renderPage('42')).rejects.toThrow('boom');
    expect(notFoundMock).not.toHaveBeenCalled();
  });
});

describe('TrackedTrainByIdPage success path', () => {
  it('renders a Delete button once the tracked train state loads', async () => {
    vi.mocked(api.getTrackedTrainById).mockResolvedValue(trackedTrainState());
    await renderPage('42');
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });
});
