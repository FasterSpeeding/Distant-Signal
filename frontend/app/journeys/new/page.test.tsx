import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { getTrainSearchDates } from '@/lib/api';
import JourneysNewPage, { metadata } from './page';

// The page mounts JourneyCreationFlow, a client component that renders
// TrackTrainForm as its leg-1 step -- that calls useRouter() at the top of
// its body, same stub `app/track/page.test.tsx` installs for the same
// reason.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/journeys/new',
  useSearchParams: () => new URLSearchParams(''),
}));

// TrackTrainForm's departures picker fires a real fetch as soon as its
// origin field holds a valid CRS, and its useSuggestions hooks fetch for
// any non-empty query -- nothing here is pre-filled, but an inert 200 keeps
// this file independent of network behaviour either way.
vi.stubGlobal(
  'fetch',
  vi.fn(async () => new Response('[]', { status: 200 })),
);

// The planner's date range is read on the server; it fails by default (the
// picker falls back to a week ahead), and every other export stays real --
// the same stub `app/plan/page.test.tsx` installs.
vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getTrainSearchDates: vi.fn(async () => {
      throw new Error('not stubbed');
    }),
  };
});

async function renderPage() {
  renderWithMantine(await JourneysNewPage());
}

// By aria-label, not role: the dropdown is still mid-transition (not yet
// "visible" to a role query) when its day buttons are rendered.
function dayButton(label: string): Promise<HTMLElement> {
  return waitFor(() => {
    const day = document.querySelector<HTMLElement>(`[aria-label="${label}"]`);
    if (!day) throw new Error(`no day ${label}`);
    return day;
  });
}

function openPlannerDatePicker() {
  fireEvent.click(screen.getByText('Plan a route for me'));
  fireEvent.focus(screen.getByLabelText('Date'));
}

describe('JourneysNewPage', () => {
  it('renders the heading, an account hint, and the leg-1 tracking form', async () => {
    await renderPage();

    expect(screen.getByRole('heading', { name: 'Track a Journey', level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/needs a Distant Signal account/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });

  it('points a visitor who only wants routes at /plan, which needs no account', async () => {
    await renderPage();
    expect(screen.getByRole('link', { name: 'Plan a journey' })).toHaveAttribute('href', '/plan');
    expect(screen.getByText(/no account needed/)).toBeInTheDocument();
  });

  // The built-in planner's picker ends where `/plan`'s does: the timetable
  // search's `to`, or a week ahead when that can't be read.
  describe("the planner's date picker", () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      vi.setSystemTime(new Date('2026-09-05T12:00:00.000Z'));
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it("ends at the search range's last day", async () => {
      vi.mocked(getTrainSearchDates).mockResolvedValueOnce({
        from: '2026-08-29',
        to: '2026-09-20',
        publishedFrom: '2026-09-04',
        publishedTo: '2026-09-20',
      });
      await renderPage();
      openPlannerDatePicker();
      expect(await dayButton('20 September 2026')).not.toBeDisabled();
      expect(await dayButton('21 September 2026')).toBeDisabled();
    });

    it('falls back to a week ahead when the range could not be read', async () => {
      await renderPage();
      openPlannerDatePicker();
      expect(await dayButton('12 September 2026')).not.toBeDisabled();
      expect(await dayButton('13 September 2026')).toBeDisabled();
    });
  });

  it('exports metadata matching its own heading', () => {
    expect(metadata.title).toContain('Track a Journey');
  });
});
