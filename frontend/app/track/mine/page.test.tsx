import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectShrinkGuarded, expectNoUnguardedNowrapBadges } from '@/test/shrinkGuard';
import MyTrackedTrainsPage from './page';
import * as api from '@/lib/api';
import type { TrackedTrainListItem, TicketListItem, SharedGroupTrain } from '@/lib/types';

vi.mock('@/lib/api');
// The not-logged-in prompt is AutoOpenLoginPrompt -> LoginPromptModal,
// which calls useLoginHref() (usePathname()/useSearchParams() under the
// hood) -- same stub AuthStatus.test.tsx and TicketPanel.test.tsx use for
// the same reason. This page also renders AttachTicketAction and
// DeleteTicketButton, both of which call useRouter() from next/navigation
// -- same workaround TicketPanel.test.tsx/TicketEntryForm.test.tsx use for
// the same reason (useRouter() throws outside an app router context).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/track/mine',
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

describe('MyTrackedTrainsPage (merged trains + tickets)', () => {
  // Every pre-existing test predates group sharing and says nothing about
  // it -- default the third fetch to "no shared trains" so each of them
  // still describes exactly the scenario it was written for. The
  // group-shared cases below override this explicitly.
  beforeEach(() => {
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([]);
  });

  it('null (not logged in): shows an auto-opened login prompt modal', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText("Log in to see the trains and tickets you're tracking.")).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack%2Fmine',
    );
  });

  it('null (not logged in): also renders a server-rendered LoginLink, not just the client-only modal', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    const link = screen.getByRole('link', { name: "Log in to see the trains and tickets you're tracking" });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrack%2Fmine');
  });

  it('no trains and no tickets: shows the empty state with a working link to /track', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/haven't tracked any trains or added any tickets yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a train' })).toHaveAttribute('href', '/track');
  });

  it('a train with no tickets: renders just the train row, no ticket content under it', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText('LNER')).not.toBeInTheDocument();
    expect(screen.queryByText('Tickets not yet attached to a train')).not.toBeInTheDocument();
  });

  it('a train with an attached ticket: renders the ticket summary and its Delay Repay estimate under the train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ trackedTrainId: 1 })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(/KGX → EDB/)).toBeInTheDocument();
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
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

    renderWithMantine(await MyTrackedTrainsPage());
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/9', { method: 'DELETE' });
    });
    vi.unstubAllGlobals();
  });

  it('multiple tickets on one train: renders every one of them, not just the first', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([
      ticket({ id: 1, trackedTrainId: 1, operator: 'LNER' }),
      ticket({ id: 2, trackedTrainId: 1, operator: 'CrossCountry', claimUrl: 'https://delayrepay.crosscountrytrains.co.uk/' }),
    ]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getAllByRole('link', { name: /See how to claim from the operator/ })).toHaveLength(2);
  });

  it('a ticket attached to a DIFFERENT tracked train does not render under this one', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 1 })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 1, trackedTrainId: 999 })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText('LNER')).not.toBeInTheDocument();
  });

  // Fix 2 (review finding C2), frontend half: an NR-primary subscription
  // (POST /Train/by-uid/{uid}/{date}/track) against a shared train with no
  // schedule data yet has BOTH pin fields null on the wire. The row must
  // still render -- degrading to "Unknown station" and a date-only label --
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
        resolutionStatus: 'pending',
        status: null,
        delayMinutes: null,
      }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Unknown station, 31 Aug 2026')).toBeInTheDocument();
    expect(screen.queryByText(/Invalid Date/)).not.toBeInTheDocument();
  });

  // Part A/B: a standalone ticket (trackedTrainId: null) not yet attached
  // to anything.
  it('a standalone (unattached) ticket: renders in its own section with an attach action and a track-a-new-train link', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ id: 1, pinOriginCrs: 'WAT', pinDestinationCrs: 'WOK' })]);
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
    renderWithMantine(await MyTrackedTrainsPage());
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
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toHaveAttribute(
      'href',
      '/track?ticketId=6',
    );
  });

  it('no tracked trains yet: the attach-to-existing-train action is not offered (nothing to attach to)', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 5, trackedTrainId: null })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.queryByLabelText('Attach to one of your tracked trains')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a new train for this ticket' })).toBeInTheDocument();
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/C21373/2026-08-31');
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ delayMinutes: 12 })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  // Task 3.6.8: `trackedTrainDisplayName` already falls back to
  // `${route}, ${when}` when no custom name is set, so the row's own
  // heading ALREADY contains "31 Aug 2026" in that case -- a second,
  // separate dimmed line repeating just the date/time would print it
  // twice on screen.
  it('no custom name: the date/time is not printed a second time under the heading', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ customName: null })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    // Scoped to this row's own Card -- the reliability digest above it can
    // independently print the same calendar date in its own "most delayed
    // journeys" list, which isn't what this assertion is about.
    const card = screen.getByRole('link', { name: /WAT → WOK/ }).closest('.mantine-Card-root') as HTMLElement;
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
    renderWithMantine(await MyTrackedTrainsPage());
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
    const { container } = renderWithMantine(await MyTrackedTrainsPage());

    expectShrinkGuarded(screen.getByText('12m late'));
    expectShrinkGuarded(screen.getByRole('button', { name: `More actions for ${customName}` }));
    expectNoUnguardedNowrapBadges(container);
  });

  it('renders station names when the backend resolved them, not just bare codes', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      train({ pinOriginName: 'London Waterloo', pinDestinationName: 'Woking' }),
    ]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/London Waterloo \(WAT\) → Woking \(WOK\)/)).toBeInTheDocument();
  });

  it('falls back to the bare code, not "null" or an empty label, when a name did not resolve', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train({ pinOriginName: null, pinDestinationName: null })]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
    expect(screen.queryByText(/null/i)).not.toBeInTheDocument();
  });

  it('renders train rows in the same order getMyTrackedTrains returned them', async () => {
    const first = train({ id: 1, pinOriginCrs: 'WAT' });
    const second = train({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent ?? '')
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });

  it('renders the reliability digest card when there is something to show', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Your reliability')).toBeInTheDocument();
  });

  it('does not render the digest card when nothingToShow (empty state)', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.queryByText('Your reliability')).not.toBeInTheDocument();
  });

  it('renders "Track a new train" and "Add a ticket" entry-point links beside the title', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: 'Track a new train' })).toHaveAttribute('href', '/track');
    expect(screen.getByRole('link', { name: 'Add a ticket' })).toHaveAttribute(
      'href',
      '/track/mine/add-ticket',
    );
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

      renderWithMantine(await MyTrackedTrainsPage());

      // The caller's own row is still there...
      expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
      // ...and the shared one now is too, with both halves of its
      // attribution.
      expect(screen.getByText(/PAD → RDG/)).toBeInTheDocument();
      expect(screen.getByText('from Family')).toBeInTheDocument();
      expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
    });

    it('a caller who tracks nothing themselves still sees trains shared with them, not the empty state', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.queryByText(/haven't tracked any trains or added any tickets yet/)).not.toBeInTheDocument();
      expect(screen.getByText(/PAD → RDG/)).toBeInTheDocument();
      expect(screen.getByText('from Family')).toBeInTheDocument();
    });

    it('a train shared into two of the caller’s groups renders once, tagged with both', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ groupId: 'g1', groupName: 'Family' }),
        sharedTrain({ groupId: 'g2', groupName: 'Commuters' }),
      ]);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getAllByText(/PAD → RDG/)).toHaveLength(1);
      expect(screen.getByText('from Family')).toBeInTheDocument();
      expect(screen.getByText('from Commuters')).toBeInTheDocument();
    });

    it('a resolved shared train links to the public /train/{uid}/{date} page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByRole('link', { name: /PAD → RDG/ })).toHaveAttribute(
        'href',
        '/train/S99999/2026-08-31',
      );
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

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByRole('link', { name: /PAD → RDG/ })).toHaveAttribute(
        'href',
        '/train/S99999/2026-08-31',
      );
    });

    it('a shared train with no uid is not linked at all — the by-id route is owner-scoped', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ resolutionStatus: 'pending', trainUid: null, status: null }),
      ]);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByText(/PAD → RDG/)).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: /PAD → RDG/ })).not.toBeInTheDocument();
      expect(screen.getByText('Pending match')).toBeInTheDocument();
    });

    it('renders the shared train’s live status and delay badges, same as an own row', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 9 })]);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByText('En route')).toBeInTheDocument();
      expect(screen.getByText('9m late')).toBeInTheDocument();
    });

    // Task 1.5 (WCAG 2.5.3): `SharedTrainListRow` pairs its heading with
    // `RowStatusBadge` in its own `Group wrap="nowrap"` (`StatusRow`),
    // separate from the own-row one covered above.
    it('gives the shared row’s status badges a shrink guard', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 9 })]);

      const { container } = renderWithMantine(await MyTrackedTrainsPage());

      expectShrinkGuarded(screen.getByText('9m late'));
      expectNoUnguardedNowrapBadges(container);
    });

    it('a sharer with no name or username is credited as "a member", never a raw user id', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ addedBy: 'sso-subject-1234', addedByName: null }),
      ]);

      renderWithMantine(await MyTrackedTrainsPage());

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

      renderWithMantine(await MyTrackedTrainsPage());

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

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByText('Shared by a member (#a1b2c3)')).toBeInTheDocument();
      expect(screen.getByText('Shared by a member (#d4e5f6)')).toBeInTheDocument();
      expect(screen.queryByText(/sso-subject-/)).not.toBeInTheDocument();
      expect(screen.queryByText(/@/)).not.toBeInTheDocument();
    });

    it('offers no rename control on someone else’s shared train', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await MyTrackedTrainsPage());

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

      renderWithMantine(await MyTrackedTrainsPage());

      // Same id as the caller's own train, so it's filtered out entirely
      // rather than rendered twice.
      expect(screen.queryByText(/PAD → RDG/)).not.toBeInTheDocument();
      expect(screen.getAllByText(/LNER/)).toHaveLength(1);
    });

    it('a shared train row carries both tags in the same row, not stranded elsewhere on the page', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

      renderWithMantine(await MyTrackedTrainsPage());

      const sharedRow = screen.getByText(/PAD → RDG/).closest('.mantine-Card-root');
      expect(sharedRow).not.toBeNull();
      expect(within(sharedRow as HTMLElement).getByText('from Family')).toBeInTheDocument();
      expect(within(sharedRow as HTMLElement).getByText('Shared by Sam')).toBeInTheDocument();
    });

    it('a null (401) shared-trains response degrades to the caller’s own list, not a crash', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue(null);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
      expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
    });

    it('renders the tracker’s own custom name on a shared row, not a recomputed route label', async () => {
      // Carrying the sharer's `customName` is the spec's headline reason
      // for sharing at all (§4: same computed default the tracker sees, or
      // their own name if they set one).
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ customName: 'School run' }),
      ]);

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByText('School run')).toBeInTheDocument();
      expect(screen.queryByText(/PAD → RDG/)).not.toBeInTheDocument();
    });

    it('puts the caller’s own rows before the shared ones, each half in its endpoint’s order', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
        sharedTrain({ trainSubscriptionId: 50, customName: 'Shared first' }),
        sharedTrain({ trainSubscriptionId: 51, customName: 'Shared second' }),
      ]);

      renderWithMantine(await MyTrackedTrainsPage());

      const rendered = [...document.querySelectorAll('.mantine-Card-root')].map(
        (card) => card.textContent ?? '',
      );
      const ownIndex = rendered.findIndex((text) => text.includes('WAT → WOK'));
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

      renderWithMantine(await MyTrackedTrainsPage());

      expect(screen.getByText(/WAT → WOK/)).toBeInTheDocument();
      expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
    });

    it('shared trains do not feed the personal reliability digest', async () => {
      vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
      vi.mocked(api.getMyTickets).mockResolvedValue([]);
      vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 30 })]);

      renderWithMantine(await MyTrackedTrainsPage());

      // The digest is "Your reliability" -- someone else's train is not
      // the caller's own punctuality record, so the card renders off the
      // caller's own trains/tickets only.
      expect(screen.getByText(/PAD → RDG/)).toBeInTheDocument();
      expect(screen.queryByText('Your reliability')).not.toBeInTheDocument();
    });
  });
});
