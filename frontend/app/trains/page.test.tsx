import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrainsPage, { metadata } from './page';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

// The page mounts TrainSearchForm, whose suggestion hooks fire real
// fetches on mount for any pre-filled, valid CRS -- give every test an
// inert 200 so none of them depend on network behaviour.
vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));

describe('TrainsPage', () => {
  // `getByRole('combobox', { name: ... })` here (rather than
  // `getByLabelText`) for the Autocomplete fields: Mantine's Autocomplete
  // always renders its options listbox in the DOM with
  // `aria-labelledby` pointing at the field's own label, even while closed
  // -- so `getByLabelText` resolves to *two* elements sharing that label
  // (the input and the listbox), not one. Station is additionally
  // `required`, which makes Mantine's InputLabel append a real (if
  // aria-hidden) " *" text node to the label, so its exact label text is
  // "Station *", not "Station" -- a second, independent reason
  // `getByLabelText('Station')` can't be used as-is. The accessible name
  // computation behind role queries excludes aria-hidden content per the
  // ARIA accname spec and targets only the `combobox` role (not the
  // `listbox`), so `getByRole('combobox', { name: ... })` lands on exactly
  // the one input in both cases. Neither issue came up in TrainSearchForm's
  // own tests: those construct the component directly with props rather
  // than through this label, and its only `getByLabelText` uses are the
  // plain TextInput time fields, which have no listbox.
  it('renders the title and the search form', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({}) }));
    expect(screen.getByRole('heading', { name: 'Find a Train' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Station' })).toBeInTheDocument();
  });

  it('pre-fills the station, origin and stops_at from the query string, uppercased', async () => {
    renderWithMantine(
      await TrainsPage({
        searchParams: Promise.resolve({ station: 'man', origin: 'eus', stops_at: 'wat' }),
      }),
    );
    expect(screen.getByRole('combobox', { name: 'Station' })).toHaveValue('MAN');
    expect(screen.getByRole('combobox', { name: 'Departing from (optional)' })).toHaveValue('EUS');
    expect(screen.getByRole('combobox', { name: 'Stops at (optional)' })).toHaveValue('WAT');
  });

  it('uses the first value when stops_at is repeated in the query string', async () => {
    renderWithMantine(
      await TrainsPage({
        searchParams: Promise.resolve({ station: 'man', stops_at: ['rdg', 'oxf'] }),
      }),
    );
    expect(screen.getByRole('combobox', { name: 'Stops at (optional)' })).toHaveValue('RDG');
  });

  // Real, unmocked DatePickerInput (this file doesn't mock '@mantine/dates',
  // unlike TrainSearchForm.test.tsx) -- same `getByDisplayValue` approach
  // HistoryRangePicker.test.tsx already uses for the same component, since
  // its rendered <input>'s accessible name/role queries don't apply the
  // way Autocomplete's combobox role does.
  it('pre-fills the date from the query string', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ station: 'man', date: '2026-09-16' }) }),
    );
    expect(screen.getByDisplayValue('2026-09-16')).toBeInTheDocument();
  });

  it('uses the first value when a query param is repeated', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ station: ['MAN', 'EDB'] }) }),
    );
    expect(screen.getByRole('combobox', { name: 'Station' })).toHaveValue('MAN');
  });

  it('shows the ticket-attach explainer copy when arriving with a ticketId', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: '7' }) }));
    expect(
      screen.getByText(
        "Find the train your saved ticket is for — it'll be attached automatically once you track it.",
      ),
    ).toBeInTheDocument();
  });

  it('carries a valid ticketId through to the manual fallback link', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: '7' }) }));
    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  // Same posture as app/track/page.tsx:16-21: a malformed value is treated
  // as absent rather than passed through as NaN.
  it('treats a non-numeric ticketId as absent', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: 'nope' }) }));
    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
    expect(
      screen.queryByText(/it'll be attached automatically once you track it/),
    ).not.toBeInTheDocument();
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    expect(metadata.title).toBe('Find a Train — Distant Signal');
  });

  it('describes network-wide scheduled-train search rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Search scheduled UK trains by any station they call at, narrowing by origin, a station they stop at later, and date. Open any result for its live status, or track it to get updates.',
    );
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // See the equivalent case in app/incidents/page.test.tsx for why the
    // mirror itself is asserted rather than just the fields' presence.
    expect(metadata.openGraph).toMatchObject({
      title: metadata.title,
      description: metadata.description,
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: metadata.title,
      description: metadata.description,
    });
  });

  it('stays static, so a per-visitor ?ticketId= can never reach a shared preview card', () => {
    // The <h1>'s own copy DOES vary with `attachTicketId` (covered above);
    // the metadata deliberately does not. A `generateMetadata` here would
    // be handed `searchParams` and could leak one visitor's ticket into a
    // cached unfurl -- this asserts the export is a plain object, not a
    // function, so that door stays shut.
    expect(typeof metadata).toBe('object');
    expect(metadata.description).not.toMatch(/ticket/i);
  });
});
