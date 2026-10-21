import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectShrinkGuarded, expectNoUnguardedNowrapBadges } from '@/test/shrinkGuard';
import JourneysPage from './page';
import * as api from '@/lib/api';
import type { TrackedTrainListItem, TicketListItem, SharedGroupTrain, JourneyListItem } from '@/lib/types';
import { byVisibleText, visibleText } from '@/test/routeText';

vi.mock('@/lib/api');
// The not-logged-in branch's LoginControl calls useLoginHref()
// (usePathname()/useSearchParams() under the hood) -- same stub AuthStatus.test.tsx and TicketPanel.test.tsx use for
// the same reason. This page also renders AttachTicketAction and
// DeleteTicketButton, both of which call useRouter() from next/navigation
// -- same workaround TicketPanel.test.tsx/TicketEntryForm.test.tsx use for
// the same reason (useRouter() throws outside an app router context).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/journeys',
  useSearchParams: () => new URLSearchParams(''),
}));

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    // null (bare-code rendering) by default -- see app/page.test.tsx's
    // `item()` for the same rationale. The name-rendering path gets its
    // own dedicated test below.
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    sharedGroupCount: 0,
    ...overrides,
    customName: overrides.customName ?? null,
  };
}

function ticket(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: 'Off-Peak Day Single',
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    originName: null,
    destinationName: null,
    source: 'manual',
    createdAt: '2026-08-31T12:00:00Z',
    serviceDate: '2026-08-31',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinScheduledDeparture: '2026-08-31T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'A12345',
    status: 'en_route',
    delayMinutes: 45,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'x' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'This is a rough, community-sourced estimate...',
    ...overrides,
    customName: overrides.customName ?? null,
  };
}

function sharedTrain(overrides: Partial<SharedGroupTrain> = {}): SharedGroupTrain {
  return {
    groupId: 'group-1',
    groupName: 'Family',
    trainSubscriptionId: 50,
    pinOriginCrs: 'PAD',
    pinDestinationCrs: 'RDG',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T07:15:00Z',
    serviceDate: '2026-08-31',
    resolutionStatus: 'resolved',
    trainUid: 'S99999',
    status: 'en_route',
    delayMinutes: null,
    customName: null,
    addedBy: 'user-2',
    addedByName: 'Sam',
    addedByTag: null,
    ...overrides,
  };
}

describe('JourneysPage', () => {
  // Every pre-existing test predates group sharing and says nothing about
  // it -- default the third fetch to "no shared trains" so each of them
  // still describes exactly the scenario it was written for. The
  // group-shared cases below override this explicitly.
  // Same reasoning for the fourth fetch, `getMyJourneys` (added for the
  // 2026-09-22 review's C1): every pre-existing test predates the journeys
  // section, so default it to "no journeys" and let the journey cases
  // below override it explicitly.
  beforeEach(() => {
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([]);
    vi.mocked(api.getMyJourneys).mockResolvedValue([]);
    // The page puts rows from a past London day under "Past". Pin
    // "today" to the fixtures' own service date so they stay active.
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-08-31T12:00:00Z'));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('null (not logged in): titles the page Journeys, with a log in button and no auto-opened modal', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await JourneysPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Journeys' })).toBeInTheDocument();
    expect(screen.getByText('Log in to see your journeys, and to keep your tickets with them.')).toBeInTheDocument();
    expect(screen.queryByText('Log in required')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Fjourneys',
    );
  });

  it('null (not logged in): offers New journey first, then the two things that need no account', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await JourneysPage());
    const newJourney = screen.getByRole('link', { name: 'New journey' });
    expect(newJourney).toHaveAttribute('href', '/journeys/new');
    expect(newJourney).toHaveClass('mantine-Button-root');
    expect(screen.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
    expect(screen.getByRole('link', { name: 'Plan a journey' })).toHaveAttribute('href', '/plan');
    expect(screen.queryByRole('link', { name: 'Add a ticket' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Upcoming' })).toBeNull();
  });

  it('no trains and no tickets: shows the empty state, under the New journey button', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText(/You have no journeys yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'New journey' })).toHaveAttribute('href', '/journeys/new');
    expect(screen.queryByRole('heading', { name: 'Upcoming' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Past' })).toBeNull();
  });

  it('a train with no tickets: renders just the train row, no ticket content under it', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
    expect(screen.queryByText('LNER')).not.toBeInTheDocument();
    expect(screen.queryByText('Tickets not yet attached to a train')).not.toBeInTheDocument();
  });

  it('a train with an attached ticket: renders the ticket summary and its Delay Repay estimate under the train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ trackedTrainId: 1 })]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(byVisibleText(/KGX to EDB/))).toBeInTheDocument();
    expect(screen.getByText(/50% of the single fare/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toHaveAttribute(
      'href',
      'https://delayrepay.lner.co.uk/delayrepayV2/',
    );
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });

  it('clicking Delete on an attached ticket row DELETEs that exact ticket id', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 9, trackedTrainId: 1 })]);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));

    renderWithMantine(await JourneysPage());
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete ticket' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/9', { method: 'DELETE' });
    });
    vi.unstubAllGlobals();
  });

  it('multiple tickets on one train: renders every one of them, not just the first', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([
      ticket({ id: 1, trackedTrainId: 1, operator: 'LNER' }),
      ticket({
        id: 2,
        trackedTrainId: 1,
        operator: 'CrossCountry',
        claimUrl: 'https://delayrepay.crosscountrytrains.co.uk/',
      }),
    ]);
    renderWithMantine(await JourneysPage());
    expect(screen.getAllByRole('link', { name: /See how to claim from the operator/ })).toHaveLength(2);
  });

  it('a ticket attached to a DIFFERENT tracked train does not render under this one', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 1 })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 1, trackedTrainId: 999 })]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
    expect(screen.queryByText('LNER')).not.toBeInTheDocument();
  });

  // Fix 2 (review finding C2), frontend half: an NR-primary subscription
  // (POST /Train/by-uid/{uid}/{date}/track) against a shared train with no
  // schedule data yet has BOTH pin fields null on the wire. The row must
  // still render -- named by its UID with a date-only label, never
  // "Unknown station" (DS-111: a normal state, not a missing station) --
  // rather than throwing or printing "Invalid Date".
  it('a subscription with null pin fields renders without crashing, degrading to a date-only label', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({
        id: 9,
        pinOriginCrs: null,
        pinOriginName: null,
        pinDestinationCrs: null,
        pinDestinationName: null,
        pinScheduledDeparture: null,
        trainUid: 'W12345',
        resolutionStatus: 'pending',
        status: null,
        delayMinutes: null,
      }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText('Train W12345, 31 Aug 2026')).toBeInTheDocument();
    expect(screen.queryByText(/Unknown station/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Invalid Date/)).not.toBeInTheDocument();
  });

  // Part A/B: a standalone ticket (trackedTrainId: null) not yet attached
  // to anything.
  it('a standalone (unattached) ticket: renders in its own section with an attach action and a track-a-new-train link', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ id: 1, pinOriginCrs: 'WAT', pinDestinationCrs: 'WOK' }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([
      ticket({
        id: 5,
        trackedTrainId: null,
        serviceDate: null,
        pinOriginCrs: null,
        pinDestinationCrs: null,
        pinScheduledDeparture: null,
        resolutionStatus: null,
        trainUid: null,
        status: null,
        delayMinutes: null,
        estimate: null,
      }),
    ]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText('Tickets not yet attached to a train')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toHaveAttribute(
      'href',
      '/track?origin=KGX&ticketId=5',
    );
    // The attach action offers the caller's own already-tracked train.
    // Mantine's Select associates its label with more than one element
    // (the visible input plus its combobox option list), so
    // getAllByLabelText (not getByLabelText) is the correct query here.
    expect(screen.getAllByLabelText('Attach to one of your tracked trains').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
  });

  it('an unattached ticket with no origin: the track-a-new-train link omits the origin param', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 6, trackedTrainId: null, originCrs: null })]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toHaveAttribute(
      'href',
      '/track?ticketId=6',
    );
  });

  it('no tracked trains yet: the attach-to-existing-train action is not offered (nothing to attach to)', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 5, trackedTrainId: null })]);
    renderWithMantine(await JourneysPage());
    expect(screen.queryByLabelText('Attach to one of your tracked trains')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toBeInTheDocument();
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByRole('link', { name: /WAT to WOK/ })).toHaveAttribute('href', '/train/C21373/2026-08-31');
    // The route keeps its arrow on screen; only the accessible text says "to".
    expect(screen.getByText(byVisibleText(/^WAT to WOK, /))).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^More actions for WAT to WOK, / })).toBeInTheDocument();
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByRole('link', { name: /WAT to WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ delayMinutes: 12 })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText('12 min late')).toBeInTheDocument();
  });

  // Task 3.6.8: `trackedTrainDisplayName` already falls back to
  // `${route}, ${when}` when no custom name is set, so the row's own
  // heading ALREADY contains "31 Aug 2026" in that case -- a second,
  // separate dimmed line repeating just the date/time would print it
  // twice on screen.
  it('no custom name: the date/time is not printed a second time under the heading', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ customName: null })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    // Scoped to this row's own Card -- the reliability digest above it can
    // independently print the same calendar date in its own "most delayed
    // journeys" list, which isn't what this assertion is about.
    const card = screen.getByRole('link', { name: /WAT to WOK/ }).closest('.mantine-Card-root') as HTMLElement;
    // Once, inside the default "route, when" heading -- not a second time
    // as its own dimmed line.
    expect(within(card).getAllByText(/31 Aug 2026/)).toHaveLength(1);
  });

  // The opposite case: once a custom name has replaced the default
  // "route, when" heading, the dimmed date/time line is the ONLY place
  // that information appears, so it must still render.
  it('with a custom name: the dimmed date/time line still renders, since the heading no longer shows it', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ customName: 'My trip to York' })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText('My trip to York')).toBeInTheDocument();
    const card = screen.getByText('My trip to York').closest('.mantine-Card-root') as HTMLElement;
    expect(within(card).getAllByText(/31 Aug 2026/)).toHaveLength(1);
  });

  // Task 1.5 (WCAG 2.5.3): the row nests a `StatusRow` (title + status
  // badge) inside an outer `StatusRow` (that inner row + the Task 3.6.7
  // overflow-kebab menu) -- both `Group wrap="nowrap"`s. Neither the badge
  // nor the kebab trigger may be crushable, even with a very long custom
  // name.
  it('gives the status badge and the overflow-kebab trigger a shrink guard, even with a very long train name', async () => {
    const customName = 'An implausibly long custom train name chosen to threaten this row’s layout';
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({
        delayMinutes: 12,
        customName,
      }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    const { container } = renderWithMantine(await JourneysPage());

    expectShrinkGuarded(screen.getByText('12 min late'));
    expectShrinkGuarded(screen.getByRole('button', { name: `More actions for ${customName}` }));
    expectNoUnguardedNowrapBadges(container);
  });

  it('renders station names when the backend resolved them, not just bare codes', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ pinOriginName: 'London Waterloo', pinDestinationName: 'Woking' }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText(byVisibleText(/London Waterloo \(WAT\) to Woking \(WOK\)/))).toBeInTheDocument();
  });

  it('falls back to the bare code, not "null" or an empty label, when a name did not resolve', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ pinOriginName: null, pinDestinationName: null })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
    expect(screen.queryByText(/null/i)).not.toBeInTheDocument();
  });

  it('renders train rows in the same order getMyTrackedTrains returned them', async () => {
    const first = train({ id: 1, pinOriginCrs: 'WAT' });
    const second = train({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent)
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });

  it('renders the reliability digest card when there is something to show', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByText('Your reliability')).toBeInTheDocument();
  });

  it('does not render the digest card when nothingToShow (empty state)', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.queryByText('Your reliability')).not.toBeInTheDocument();
  });

  it('heads the page Journeys, with New journey and every other way to start beside it', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await JourneysPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Journeys' })).toBeInTheDocument();
    const newJourney = screen.getByRole('link', { name: 'New journey' });
    expect(newJourney).toHaveAttribute('href', '/journeys/new');
    expect(newJourney).toHaveClass('mantine-Button-root');
    expect(screen.queryByRole('link', { name: 'Track a journey' })).toBeNull();
    expect(screen.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
    expect(screen.getByRole('link', { name: 'Add a ticket' })).toHaveAttribute('href', '/track/mine/add-ticket');
    expect(screen.getByRole('link', { name: 'Journey templates' })).toHaveAttribute('href', '/journeys/templates');
  });

  // The reported bug: a train another member shared into a group the
  // caller belongs to never reached this page at all -- it only existed on
  // `/groups/{id}`. These cover it appearing here, and being tagged with
  // where it came from so it can't be mistaken for one the caller tracked.
  describe('group-shared trains', () => {
    it('renders a group-shared train alongside the caller’s own, tagged with its group and sharer', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await JourneysPage());

      // The caller's own row is still there...
      expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
      // ...and the shared one now is too, with both halves of its
      // attribution.
      expect(screen.getByText(byVisibleText(/PAD to RDG/))).toBeInTheDocument();
      expect(screen.getByText('From Family')).toBeInTheDocument();
      expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
    });

    it('a caller who tracks nothing themselves still sees trains shared with them, not the empty state', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await JourneysPage());

      expect(screen.queryByText(/You have no journeys yet/)).not.toBeInTheDocument();
      expect(screen.getByText(byVisibleText(/PAD to RDG/))).toBeInTheDocument();
      expect(screen.getByText('From Family')).toBeInTheDocument();
    });

    it('a train shared into two of the caller’s groups renders once, tagged with both', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ groupId: 'g1', groupName: 'Family' }),
        sharedTrain({ groupId: 'g2', groupName: 'Commuters' }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getAllByText(byVisibleText(/PAD to RDG/))).toHaveLength(1);
      expect(screen.getByText('From Family')).toBeInTheDocument();
      expect(screen.getByText('From Commuters')).toBeInTheDocument();
    });

    it('a resolved shared train links to the public /train/{uid}/{date} page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByRole('link', { name: /PAD to RDG/ })).toHaveAttribute('href', '/train/S99999/2026-08-31');
    });

    it('a shared train with a uid but a not-yet-resolved status is still linked', async () => {
      // `trains_id` (and so `trainUid`) lands well before the status
      // reaches `resolved`, and `/train/{uid}/{date}` is public the whole
      // time -- gating the link on `resolved` would render these as dead
      // text for no reason. The caller's own rows can afford the stricter
      // test only because they have an owner-scoped by-id fallback.
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ resolutionStatus: 'schedule_matched', trainUid: 'S99999', status: null }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByRole('link', { name: /PAD to RDG/ })).toHaveAttribute('href', '/train/S99999/2026-08-31');
    });

    it('a shared train with no uid is not linked at all — the by-id route is owner-scoped', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ resolutionStatus: 'pending', trainUid: null, status: null }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/PAD to RDG/))).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: /PAD to RDG/ })).not.toBeInTheDocument();
      // The journey rollup's word for a train not yet reporting.
      expect(screen.getByText('Awaiting first report')).toBeInTheDocument();
    });

    // The reported mismatch: the owner's own row said "Planned" while the
    // same train, shared from a group, said "Pending match". The shared
    // row now carries the owner's `trackingPhase` and `serviceMode`.
    it('a shared train reads the same tracking phase and mode words as an own row', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({
          trainSubscriptionId: 801,
          resolutionStatus: 'pending',
          status: null,
          delayMinutes: null,
          trackingPhase: 'planned',
        }),
        sharedTrain({
          trainSubscriptionId: 802,
          resolutionStatus: 'pending',
          trainUid: 'S99998',
          status: null,
          delayMinutes: null,
          trackingPhase: 'noTimetable',
        }),
        sharedTrain({
          trainSubscriptionId: 803,
          resolutionStatus: 'schedule_matched',
          trainUid: 'B99999',
          status: null,
          delayMinutes: null,
          trackingPhase: 'timetableOnly',
          serviceMode: 'bus',
          liveTracking: false,
        }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText('Planned')).toBeInTheDocument();
      expect(screen.getByText('Not in the timetable')).toBeInTheDocument();
      expect(screen.getByText('Bus service')).toBeInTheDocument();
      expect(screen.getByText('Timetabled only')).toBeInTheDocument();
      expect(screen.queryByText('Pending match')).not.toBeInTheDocument();
    });

    it('renders the shared train’s live status and delay badges, same as an own row', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 9 })]);

      renderWithMantine(await JourneysPage());

      // The delay badge alone, as on a journey row: no separate "En route".
      expect(screen.queryByText('En route')).not.toBeInTheDocument();
      expect(screen.getByText('9 min late')).toBeInTheDocument();
    });

    // Task 1.5 (WCAG 2.5.3): `SharedTrainListRow` pairs its heading with
    // `RowStatusBadge` in its own `Group wrap="nowrap"` (`StatusRow`),
    // separate from the own-row one covered above.
    it('gives the shared row’s status badges a shrink guard', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 9 })]);

      const { container } = renderWithMantine(await JourneysPage());

      expectShrinkGuarded(screen.getByText('9 min late'));
      expectNoUnguardedNowrapBadges(container);
    });

    it('a sharer with no name or username is credited as "a member", never a raw user id', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ addedBy: 'sso-subject-1234', addedByName: null }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText('Shared by a member')).toBeInTheDocument();
      expect(screen.queryByText(/sso-subject-1234/)).not.toBeInTheDocument();
    });

    /** A BLANK name, not a null one -- what an identity provider with no
     * name on file for the sharer actually sends. `??` treats `''` as a
     * usable label, so this row read "Shared by " with nothing after it.
     * The backend normalizes blanks away now; this guards the rows written
     * before it did, exactly as `/groups/{id}`'s own row does. */
    it('a sharer whose name is blank rather than null is still credited as "a member"', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ addedBy: 'sso-subject-1234', addedByName: '   ' }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText('Shared by a member')).toBeInTheDocument();
    });

    /** Two unnameable sharers -- the Entra-ID case, where the username
     * claim IS the user's email-shaped UPN so the backend can name nobody
     * -- must still credit two different people, or this list says every
     * shared train came from the same anonymous "a member". The tag is
     * derived from the sharer's opaque id, never from their address (see
     * `crates/api/src/data/users.rs`'s `MemberDisplay`), and matches the
     * one their row carries in the group's own member list. */
    it('credits two unnameable sharers distinguishably, still never an email or raw id', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({
          trainSubscriptionId: 50,
          addedBy: 'sso-subject-1234',
          addedByName: null,
          addedByTag: 'a1b2c3',
        }),
        sharedTrain({
          trainSubscriptionId: 51,
          pinOriginCrs: 'WOK',
          pinDestinationCrs: 'WAT',
          addedBy: 'sso-subject-5678',
          addedByName: null,
          addedByTag: 'd4e5f6',
        }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText('Shared by a member (#a1b2c3)')).toBeInTheDocument();
      expect(screen.getByText('Shared by a member (#d4e5f6)')).toBeInTheDocument();
      expect(screen.queryByText(/sso-subject-/)).not.toBeInTheDocument();
      expect(screen.queryByText(/@/)).not.toBeInTheDocument();
    });

    it('offers no rename control on someone else’s shared train', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await JourneysPage());

      // Exactly one overflow-kebab (Rename/Stop tracking) trigger on the
      // page: the caller's own row's -- `SharedTrainListRow` never renders
      // one at all (see its own doc comment: none of its controls apply to
      // a train the caller doesn't own).
      expect(screen.getAllByRole('button', { name: /^More actions for/ })).toHaveLength(1);
    });

    it('never shows a ticket under a shared train, even when the caller has one for the same id', async () => {
      // The shared train's `trainSubscriptionId` deliberately collides with
      // one of the caller's OWN ticket's `trackedTrainId` here: tickets are
      // keyed off the caller's own tracked-train ids, and a shared row must
      // never pick one up by id collision (spec §4 forbids a shared train
      // carrying ticket data at all).
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 1 })]);
      vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 3, trackedTrainId: 1 })]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ trainSubscriptionId: 1, pinOriginCrs: 'PAD', pinDestinationCrs: 'RDG' }),
      ]);

      renderWithMantine(await JourneysPage());

      // Same id as the caller's own train, so it's filtered out entirely
      // rather than rendered twice.
      expect(screen.queryByText(byVisibleText(/PAD to RDG/))).not.toBeInTheDocument();
      expect(screen.getAllByText(/LNER/)).toHaveLength(1);
    });

    it('a shared train row carries both tags in the same row, not stranded elsewhere on the page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await JourneysPage());

      const sharedRow = screen.getByText(byVisibleText(/PAD to RDG/)).closest('.mantine-Card-root');
      expect(sharedRow).not.toBeNull();
      expect(within(sharedRow as HTMLElement).getByText('From Family')).toBeInTheDocument();
      expect(within(sharedRow as HTMLElement).getByText('Shared by Sam')).toBeInTheDocument();
    });

    it('a null (401) shared-trains response degrades to the caller’s own list, not a crash', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue(null);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
      expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
    });

    it('renders the tracker’s own custom name on a shared row, not a recomputed route label', async () => {
      // Carrying the sharer's `customName` is the spec's headline reason
      // for sharing at all (§4: same computed default the tracker sees, or
      // their own name if they set one).
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ customName: 'School run' })]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText('School run')).toBeInTheDocument();
      expect(screen.queryByText(byVisibleText(/PAD to RDG/))).not.toBeInTheDocument();
    });

    it('puts the caller’s own rows before the shared ones, each half in its endpoint’s order', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ trainSubscriptionId: 50, customName: 'Shared first' }),
        sharedTrain({ trainSubscriptionId: 51, customName: 'Shared second' }),
      ]);

      renderWithMantine(await JourneysPage());

      const rendered = [...document.querySelectorAll('.mantine-Card-root')].map((card) => visibleText(card));
      const ownIndex = rendered.findIndex((text) => text.includes('WAT to WOK'));
      const firstSharedIndex = rendered.findIndex((text) => text.includes('Shared first'));
      const secondSharedIndex = rendered.findIndex((text) => text.includes('Shared second'));
      expect(ownIndex).toBeGreaterThanOrEqual(0);
      expect(ownIndex).toBeLessThan(firstSharedIndex);
      expect(firstSharedIndex).toBeLessThan(secondSharedIndex);
    });

    it('a failing shared-trains fetch still renders the caller’s own list rather than erroring the page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockRejectedValue(new Error('API request failed: 500'));

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
      expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
    });

    it('shared trains do not feed the personal reliability digest', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 30 })]);

      renderWithMantine(await JourneysPage());

      // The digest is "Your reliability" -- someone else's train is not
      // the caller's own punctuality record, so the card renders off the
      // caller's own trains/tickets only.
      expect(screen.getByText(byVisibleText(/PAD to RDG/))).toBeInTheDocument();
      expect(screen.queryByText('Your reliability')).not.toBeInTheDocument();
    });
  });
  // 2026-09-22 UX review, C1: `getMyJourneys()` existed in lib/api.ts with
  // zero callers, so a journey created by time-window search was
  // unreachable the moment its page was closed.
  describe('journeys (C1)', () => {
    function journey(overrides: Partial<JourneyListItem> = {}): JourneyListItem {
      return {
        id: 169,
        customName: null,
        createdAt: '2026-09-22T18:13:54Z',
        legId: 194,
        originCrs: 'KGX',
        destinationCrs: 'EDB',
        serviceDate: '2026-09-22',
        originName: null,
        destinationName: null,
        matchMode: 'unmatched',
        trainSubscriptionId: null,
        resolutionStatus: null,
        status: null,
        delayMinutes: null,
        ...overrides,
      };
    }

    it('lists an unmatched window-search journey, linking to its own page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey()]);

      renderWithMantine(await JourneysPage());

      const upcoming = screen.getByRole('region', { name: 'Upcoming' });
      const link = within(upcoming).getByRole('link', { name: /KGX to EDB/ });
      expect(link).toHaveAttribute('href', '/journeys/169');
      expect(screen.getByText('Needs a train picked')).toBeInTheDocument();
    });

    it('does not show the "you have not tracked anything" empty state when the only object is a journey', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey()]);

      renderWithMantine(await JourneysPage());

      expect(screen.queryByText(/You have no journeys yet/)).not.toBeInTheDocument();
    });

    it('renders station names and a formatted date when the backend resolved them', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({ originName: 'London Kings Cross', destinationName: 'Edinburgh' }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/London Kings Cross \(KGX\) to Edinburgh \(EDB\)/))).toBeInTheDocument();
      // `formatDate`, not the raw ISO `serviceDate` the wire carries.
      expect(screen.queryByText(/2026-09-22/)).not.toBeInTheDocument();
    });

    // The reported bug: a planned journey with no tracking data yet read as
    // running from its origin to its FIRST leg's destination. The row's
    // `originCrs`/`destinationCrs` are the current leg's; the name comes
    // from the whole-journey fields.
    it('names a planned three-leg journey by its real final destination, with a leg count', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({
          originCrs: 'KGX',
          originName: 'London Kings Cross',
          destinationCrs: 'YRK',
          destinationName: 'York',
          firstOriginCrs: 'KGX',
          firstOriginName: 'London Kings Cross',
          lastDestinationCrs: 'EDB',
          lastDestinationName: 'Edinburgh',
          firstServiceDate: '2026-09-22',
          legCount: 3,
        }),
      ]);

      renderWithMantine(await JourneysPage());

      const link = screen.getByRole('link', { name: 'London Kings Cross (KGX) to Edinburgh (EDB), 22 Sep 2026' });
      expect(link).toHaveAttribute('href', '/journeys/169');
      expect(screen.getByText('3 legs')).toBeInTheDocument();
      expect(screen.queryByText(/to York/)).not.toBeInTheDocument();
    });

    it('names a journey part-way through by its whole route and first day, not the current leg', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({
          originCrs: 'YRK',
          destinationCrs: 'EDB',
          serviceDate: '2026-09-23',
          firstOriginCrs: 'KGX',
          lastDestinationCrs: 'EDB',
          firstServiceDate: '2026-09-22',
          legCount: 2,
          matchMode: 'auto',
          trainSubscriptionId: 295,
          status: 'en_route',
          delayMinutes: 0,
        }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByRole('link', { name: 'KGX to EDB, 22 Sep 2026' })).toBeInTheDocument();
      expect(screen.getByText('2 legs')).toBeInTheDocument();
    });

    it('shows a custom name, with the whole route and date beneath it', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({
          customName: 'Trip north',
          firstOriginCrs: 'KGX',
          lastDestinationCrs: 'EDB',
          firstServiceDate: '2026-09-22',
          legCount: 1,
        }),
      ]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByRole('link', { name: 'Trip north' })).toHaveAttribute('href', '/journeys/169');
      expect(screen.getByText('KGX to EDB, 22 Sep 2026')).toBeInTheDocument();
    });

    it('falls back to the current leg for an older backend without the whole-journey fields', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey()]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByRole('link', { name: 'KGX to EDB, 22 Sep 2026' })).toBeInTheDocument();
    });

    it("reflects a matched leg's delay in the journey row's badge", async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({ matchMode: 'auto', trainSubscriptionId: 295, status: 'en_route', delayMinutes: 22 }),
      ]);

      renderWithMantine(await JourneysPage());

      // The delay badge, worded the same as on a lone train's row.
      expect(screen.getByText('22 min late')).toBeInTheDocument();
    });

    it('gives a journey and a lone train in the same state the same badge', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 7, delayMinutes: 6, journeyIds: [] })]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({ matchMode: 'auto', trainSubscriptionId: 295, status: 'en_route', delayMinutes: 6 }),
      ]);

      renderWithMantine(await JourneysPage());

      const badges = screen.getAllByText('6 min late');
      expect(badges).toHaveLength(2);
      expect(badges[0]?.closest('[data-delay-badge]')).toHaveAttribute('data-delay-badge', 'late');
      expect(badges[1]?.closest('[data-delay-badge]')).toHaveAttribute('data-delay-badge', 'late');
    });

    // DS-115: Upcoming and Past, with one cut-off. Fixtures come in the api's
    // travel order (`GET /Journeys/mine`, `GET /Train/mine`): still to come
    // or under way, soonest first, then travelled, most recent first.
    describe('Upcoming and Past', () => {
      const region = (name: string) => screen.getByRole('region', { name });

      it('splits journeys and trains into Upcoming and Past, keeping cancellations red', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
          train({ id: 8, pinOriginCrs: 'GLD', pinDestinationCrs: 'WOK', journeyIds: [] }),
          train({ id: 7, status: 'completed', delayMinutes: 3, journeyIds: [] }),
        ]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getMyJourneys).mockResolvedValue([
          journey({ id: 171, originCrs: 'YRK', destinationCrs: 'NCL' }),
          journey({
            id: 170,
            serviceDate: '2026-08-30',
            legCount: 2,
            matchMode: 'auto',
            trainSubscriptionId: 296,
            status: 'cancelled',
          }),
        ]);

        renderWithMantine(await JourneysPage());

        const upcoming = region('Upcoming');
        expect(within(upcoming).getByRole('link', { name: /YRK to NCL/ })).toHaveAttribute('href', '/journeys/171');
        expect(within(upcoming).getByText('Needs a train picked')).toBeInTheDocument();
        expect(within(upcoming).getByText(byVisibleText(/GLD to WOK/))).toBeInTheDocument();

        const past = region('Past');
        expect(within(past).getByRole('link', { name: /KGX to EDB/ })).toHaveAttribute('href', '/journeys/170');
        expect(within(past).getByText('Part cancelled').closest('[data-state-badge]')).toHaveAttribute(
          'data-state-badge',
          'critical',
        );
        expect(within(past).getByText('Completed')).toBeInTheDocument();
        expect(within(past).getByText('3 min late')).toBeInTheDocument();
        expect(within(past).queryByText(byVisibleText(/GLD to WOK/))).toBeNull();
        expect(within(upcoming).queryByRole('link', { name: /KGX to EDB/ })).toBeNull();
        expect(screen.queryByRole('heading', { name: 'Finished' })).toBeNull();
      });

      it('puts Your reliability below Upcoming and above Past', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
          train({ id: 8, pinOriginCrs: 'GLD', pinDestinationCrs: 'WOK', journeyIds: [] }),
          train({ id: 7, status: 'completed', delayMinutes: 3, journeyIds: [] }),
        ]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);

        renderWithMantine(await JourneysPage());

        const upcoming = screen.getByRole('heading', { name: 'Upcoming' });
        const reliability = screen.getByRole('heading', { name: 'Your reliability' });
        const past = screen.getByRole('heading', { name: 'Past' });
        expect(upcoming.compareDocumentPosition(reliability) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(reliability.compareDocumentPosition(past) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        // Neither list holds it.
        expect(within(region('Upcoming')).queryByRole('heading', { name: 'Your reliability' })).toBeNull();
        expect(within(region('Past')).queryByRole('heading', { name: 'Your reliability' })).toBeNull();
      });

      it('still shows Upcoming, saying it is empty, when everything is past', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getMyJourneys).mockResolvedValue([journey({ id: 170, serviceDate: '2026-08-20' })]);

        renderWithMantine(await JourneysPage());

        expect(within(region('Upcoming')).getByText('No upcoming journeys.')).toBeInTheDocument();
        expect(within(region('Past')).getByRole('link', { name: /KGX to EDB/ })).toBeInTheDocument();
      });

      it('leaves Past out when nothing is over yet', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getMyJourneys).mockResolvedValue([journey()]);

        renderWithMantine(await JourneysPage());

        expect(screen.getByRole('heading', { name: 'Upcoming' })).toBeInTheDocument();
        expect(screen.queryByRole('heading', { name: 'Past' })).toBeNull();
        expect(screen.queryByText('No upcoming journeys.')).toBeNull();
      });

      it('counts a journey whose every leg has arrived as past, even on its own day', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getMyJourneys).mockResolvedValue([
          journey({ id: 180, originCrs: 'YRK', destinationCrs: 'NCL', serviceDate: '2026-08-31', status: 'en_route' }),
          journey({
            id: 181,
            serviceDate: '2026-08-31',
            legCount: 2,
            trainLegCount: 2,
            cancelledLegCount: 0,
            unmatchedLegCount: 0,
            finishedLegCount: 2,
            startedLegCount: 2,
            matchMode: 'auto',
            trainSubscriptionId: 296,
            resolutionStatus: 'resolved',
            status: 'completed',
          }),
        ]);

        renderWithMantine(await JourneysPage());

        expect(within(region('Upcoming')).getByRole('link', { name: /YRK to NCL/ })).toBeInTheDocument();
        expect(within(region('Past')).getByRole('link', { name: /KGX to EDB/ })).toHaveAttribute(
          'href',
          '/journeys/181',
        );
      });

      // The api orders by its own arrival rule, which a list row has no
      // times to repeat: a row it put after a later-starting one has been
      // travelled, whatever its last status said.
      it('reads the cut-off from the api order when a row has no times of its own', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getMyJourneys).mockResolvedValue([
          journey({ id: 190, originCrs: 'YRK', destinationCrs: 'NCL', serviceDate: '2026-09-01' }),
          // Today's, still "en route" (no arrival report yet), but after
          // tomorrow's in the api's order: the api has it as arrived.
          journey({
            id: 191,
            serviceDate: '2026-08-31',
            matchMode: 'auto',
            trainSubscriptionId: 296,
            resolutionStatus: 'resolved',
            status: 'en_route',
          }),
          // And everything after a past row is past.
          journey({
            id: 192,
            originCrs: 'PAD',
            destinationCrs: 'RDG',
            serviceDate: '2026-08-31',
            matchMode: 'auto',
            trainSubscriptionId: 297,
            resolutionStatus: 'resolved',
            status: 'en_route',
          }),
        ]);

        renderWithMantine(await JourneysPage());

        const upcoming = region('Upcoming');
        const past = region('Past');
        expect(within(upcoming).getByRole('link', { name: /YRK to NCL/ })).toBeInTheDocument();
        expect(within(past).getByRole('link', { name: /KGX to EDB/ })).toHaveAttribute('href', '/journeys/191');
        expect(within(past).getByRole('link', { name: /PAD to RDG/ })).toHaveAttribute('href', '/journeys/192');
      });

      it('keeps a journey that starts after today, and every row before it, upcoming', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getMyJourneys).mockResolvedValue([
          // A trip whose first leg was yesterday and whose return is next
          // week: not over, so the api lists it first.
          journey({ id: 200, firstServiceDate: '2026-08-30', serviceDate: '2026-08-30', legCount: 2 }),
          journey({ id: 201, originCrs: 'YRK', destinationCrs: 'NCL', serviceDate: '2026-09-05' }),
        ]);

        renderWithMantine(await JourneysPage());

        const upcoming = region('Upcoming');
        expect(within(upcoming).getByRole('link', { name: /KGX to EDB/ })).toHaveAttribute('href', '/journeys/200');
        expect(within(upcoming).getByRole('link', { name: /YRK to NCL/ })).toBeInTheDocument();
        expect(screen.queryByRole('heading', { name: 'Past' })).toBeNull();
      });

      it('places each shared train by its own status, after the caller’s own rows', async () => {
        vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ journeyIds: [] })]);
        vi.mocked(api.getMyTickets).mockResolvedValue([]);
        vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
          sharedTrain({
            trainSubscriptionId: 60,
            serviceDate: '2026-08-20',
            pinOriginCrs: 'BTN',
            pinDestinationCrs: 'VIC',
          }),
          sharedTrain({ trainSubscriptionId: 61 }),
        ]);

        renderWithMantine(await JourneysPage());

        const upcoming = region('Upcoming');
        expect(within(upcoming).getByText(byVisibleText(/PAD to RDG/))).toBeInTheDocument();
        const ownRow = within(upcoming).getByText(byVisibleText(/WAT to WOK/));
        const sharedRow = within(upcoming).getByText(byVisibleText(/PAD to RDG/));
        expect(ownRow.compareDocumentPosition(sharedRow) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(within(region('Past')).getByText(byVisibleText(/BTN to VIC/))).toBeInTheDocument();
      });
    });

    it("does not list a journey leg's own train a second time as a standalone tracked train", async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 295 })]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([
        journey({ matchMode: 'auto', trainSubscriptionId: 295, status: 'en_route', delayMinutes: 0 }),
      ]);

      renderWithMantine(await JourneysPage());

      // The journey row is the one that survives; the bare train row for
      // the same subscription is suppressed.
      expect(screen.getByRole('link', { name: /KGX to EDB/ })).toHaveAttribute('href', '/journeys/169');
      expect(screen.queryByText(byVisibleText(/WAT to WOK/))).not.toBeInTheDocument();
    });

    // The reported bug: legs 2 onward came back as standalone tracked
    // trains, because only the current leg's train was matched.
    it("folds every leg's train into its journey, not just the current leg's", async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
        train({ id: 295, journeyIds: [169] }),
        train({ id: 296, pinOriginCrs: 'YRK', pinDestinationCrs: 'NCL', journeyIds: [169] }),
        train({ id: 297, pinOriginCrs: 'NCL', pinDestinationCrs: 'EDB', journeyIds: [169] }),
        train({ id: 7, pinOriginCrs: 'PAD', pinDestinationCrs: 'RDG', journeyIds: [] }),
      ]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey({ matchMode: 'manual', trainSubscriptionId: 295 })]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByRole('link', { name: /KGX to EDB/ })).toHaveAttribute('href', '/journeys/169');
      expect(screen.queryByText(byVisibleText(/WAT to WOK/))).not.toBeInTheDocument();
      expect(screen.queryByText(byVisibleText(/YRK to NCL/))).not.toBeInTheDocument();
      expect(screen.queryByText(byVisibleText(/NCL to EDB/))).not.toBeInTheDocument();
      expect(screen.getByText(byVisibleText(/PAD to RDG/))).toBeInTheDocument();
    });

    it("keeps a folded train's ticket on the page, under its journey", async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 296, journeyIds: [169] })]);
      vi.mocked(api.getMyTickets).mockResolvedValue([
        ticket({ id: 41, trackedTrainId: 296, operator: 'Grand Central' }),
      ]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey()]);

      renderWithMantine(await JourneysPage());

      expect(screen.queryByText(byVisibleText(/WAT to WOK/))).not.toBeInTheDocument();
      expect(screen.getByText(/Grand Central/)).toBeInTheDocument();
    });

    it('lists a train whose journey is not on the page as a standalone row, rather than hiding it', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 296, journeyIds: [12] })]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey()]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
    });

    it('still lists tracked trains that belong to no journey', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 7 })]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockResolvedValue([journey({ matchMode: 'auto', trainSubscriptionId: 295 })]);

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
    });

    it('survives a failing journeys fetch without losing the rest of the page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getMyJourneys).mockRejectedValue(new Error('API request failed: 500'));

      renderWithMantine(await JourneysPage());

      expect(screen.getByText(byVisibleText(/WAT to WOK/))).toBeInTheDocument();
      expect(within(screen.getByRole('region', { name: 'Upcoming' })).queryAllByRole('link', { name: /^KGX/ })).toEqual(
        [],
      );
    });
  });
});
