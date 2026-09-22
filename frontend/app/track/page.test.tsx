import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackPage, { metadata } from './page';
// Namespace import alongside the named one purely so the "no
// generateMetadata export" case below can test the module's shape -- same
// pattern app/trains/page.test.tsx uses for the same reason.
import * as pageModule from './page';

// The page mounts TrackTrainForm, a client component that calls
// useRouter() at the top of its body -- same stub app/trains/page.test.tsx
// and app/stations/page.test.tsx install for their own forms.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/track',
  useSearchParams: () => new URLSearchParams(''),
}));

// TrackTrainForm's departures picker fires a real fetch as soon as its
// origin field holds a valid CRS, and its useSuggestions hooks fetch for
// any non-empty query. Nothing here is pre-filled, but an inert 200 keeps
// this file independent of network behaviour either way.
vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));

describe('TrackPage', () => {
  it('renders the heading, the default subtitle and the tracking form', async () => {
    renderWithMantine(await TrackPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByRole('heading', { name: 'Track a Train', level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/Pin a specific train to see its live position/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });

  // Review §2.16: the "Track this train" button above is shown to every
  // visitor, logged in or not, with nothing hinting that saving a pin needs
  // an account -- this note is that hint.
  it('hints that tracking a train needs an account', async () => {
    renderWithMantine(await TrackPage({ searchParams: Promise.resolve({}) }));
    expect(screen.getByText(/needs a Distant Signal account/)).toBeInTheDocument();
  });

  it('swaps in the ticket-specific subtitle for a valid ?ticketId=', async () => {
    renderWithMantine(await TrackPage({ searchParams: Promise.resolve({ ticketId: '42' }) }));

    expect(screen.getByText(/Find or track the train your saved ticket is for/)).toBeInTheDocument();
  });

  // Review §2.1/I21: previously nothing in the app could link straight to
  // window mode. `?mode=window` mirrors the existing `?origin=` pattern.
  it('starts the form in window mode for ?mode=window', async () => {
    renderWithMantine(await TrackPage({ searchParams: Promise.resolve({ mode: 'window' }) }));

    expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
    expect(screen.getByText(/Not sure which train yet\?/)).toBeInTheDocument();
  });

  it('falls back to pick mode for an unrecognised ?mode=', async () => {
    renderWithMantine(await TrackPage({ searchParams: Promise.resolve({ mode: 'bogus' }) }));

    expect(screen.getByRole('radio', { name: 'I know the train' })).toBeChecked();
  });

  it('pre-fills destination and window bounds for ?mode=window&destination=&departAfter=...', async () => {
    renderWithMantine(
      await TrackPage({
        searchParams: Promise.resolve({
          mode: 'window',
          origin: 'wat',
          destination: 'rdg',
          departAfter: '08:00',
          departBefore: '09:00',
          arriveAfter: '11:00',
          arriveBefore: '10:00',
        }),
      }),
    );

    // Role/label-based queries, not `getByDisplayValue` -- a
    // `getByDisplayValue` match only confirms a value appears SOMEWHERE on
    // the rendered form, not that it landed in the specific field it's
    // supposed to (a param<->prop transposition bug, e.g. swapping
    // `departAfter`/`arriveBefore`, would still pass). `Destination
    // station` is a Mantine `Autocomplete`, which `getByLabelText` doesn't
    // reliably match -- same `combobox` pattern
    // `TrackTrainForm.test.tsx`'s own window-mode tests already use. The
    // four time bounds use `getByLabelText` with their exact accessible
    // names, confirmed reliable on these `TimeInput` fields elsewhere in
    // that same suite.
    expect(screen.getByRole('combobox', { name: /^Destination station$/ })).toHaveValue('RDG');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('08:00');
    expect(screen.getByLabelText('Latest departure (optional)')).toHaveValue('09:00');
    expect(screen.getByLabelText('Earliest arrival (optional)')).toHaveValue('11:00');
    expect(screen.getByLabelText('Latest arrival (optional)')).toHaveValue('10:00');
  });

  it('pre-fills the pin-mode destination for a plain ?destination= with no ?mode=', async () => {
    renderWithMantine(
      await TrackPage({ searchParams: Promise.resolve({ origin: 'wat', destination: 'rdg' }) }),
    );

    expect(screen.getByDisplayValue('RDG')).toBeInTheDocument();
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    expect(metadata.title).toBe('Track a Train — Distant Signal');
  });

  it('describes pinning one train rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Pin a specific train — picked from the upcoming departures at its origin station, or entered by hand — to see its live position, delay and next calling point as Network Rail reports it. Not sure which train yet? Search a time window instead and pick from the matches.',
    );
  });

  it("doesn't call the picker's departures live, since it falls back to the scheduled timetable", () => {
    // TrackTrainForm's CIF branch says outright that it is "showing the
    // scheduled timetable instead — this is not live running information
    // and may be up to 30 minutes out of date" for any station LDBWS has
    // no board for, so "the live departure board" would be a promise the
    // page can't always keep. "live position" (the pin itself, which IS
    // live) is a different claim and deliberately kept.
    expect(metadata.description).toMatch(/upcoming departures/);
    expect(metadata.description).not.toMatch(/live departure/i);
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // See the equivalent case in app/incidents/page.test.tsx for why the
    // mirror is asserted against literals rather than against
    // `metadata.title`/`.description`.
    expect(metadata.openGraph).toMatchObject({
      title: 'Track a Train — Distant Signal',
      description:
        'Pin a specific train — picked from the upcoming departures at its origin station, or entered by hand — to see its live position, delay and next calling point as Network Rail reports it. Not sure which train yet? Search a time window instead and pick from the matches.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: 'Track a Train — Distant Signal',
      description:
        'Pin a specific train — picked from the upcoming departures at its origin station, or entered by hand — to see its live position, delay and next calling point as Network Rail reports it. Not sure which train yet? Search a time window instead and pick from the matches.',
    });
  });

  it('stays static, so a per-visitor ?ticketId=/?origin= can never reach a shared preview card', () => {
    // The subtitle's copy DOES vary with `attachTicketId` (covered above);
    // the metadata deliberately does not. Next hands `generateMetadata`
    // the same `searchParams` this page component gets, so adding one here
    // would put one visitor's saved ticket -- or the station they happened
    // to arrive from -- within reach of a cached, shared unfurl. Asserted
    // as "this module exports no generateMetadata at all" --
    // `typeof metadata === 'object'` would NOT catch it, since Next's
    // function form is a separate, differently-named export that can sit
    // alongside this one.
    expect('generateMetadata' in pageModule).toBe(false);
  });

  it('says nothing about tickets, the one searchParam with a page-visible branch', () => {
    // Belt-and-braces alongside the structural check above: even a future
    // hand-written static description must not describe the
    // `?ticketId=`-only flow, since an unfurler bot never carries that
    // param and would be previewing a page state it cannot reach.
    expect(JSON.stringify(metadata)).not.toMatch(/ticket/i);
  });
});
