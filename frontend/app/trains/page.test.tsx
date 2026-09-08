import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrainsPage from './page';

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
  // `getByLabelText`) for both Autocomplete fields: Mantine's Autocomplete
  // always renders its options listbox in the DOM with
  // `aria-labelledby` pointing at the field's own label, even while closed
  // -- so `getByLabelText` resolves to *two* elements sharing that label
  // (the input and the listbox), not one. Destination is additionally
  // `required`, which makes Mantine's InputLabel append a real (if
  // aria-hidden) " *" text node to the label, so its exact label text is
  // "Destination station *", not "Destination station" -- a second,
  // independent reason `getByLabelText('Destination station')` can't be
  // used as-is. The accessible name computation behind role queries
  // excludes aria-hidden content per the ARIA accname spec and targets only
  // the `combobox` role (not the `listbox`), so `getByRole('combobox', {
  // name: ... })` lands on exactly the one input in both cases. Neither
  // issue came up in TrainSearchForm's own tests: those construct the
  // component directly with props rather than through this label, and its
  // only `getByLabelText` uses are the plain TextInput time fields, which
  // have no listbox.
  it('renders the title and the search form', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({}) }));
    expect(screen.getByRole('heading', { name: 'Find a Train' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Destination station' })).toBeInTheDocument();
  });

  it('pre-fills the destination and origin from the query string, uppercased', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ destination: 'man', origin: 'eus' }) }),
    );
    expect(screen.getByRole('combobox', { name: 'Destination station' })).toHaveValue('MAN');
    expect(screen.getByRole('combobox', { name: 'Departing from (optional)' })).toHaveValue('EUS');
  });

  it('uses the first value when a query param is repeated', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ destination: ['MAN', 'EDB'] }) }),
    );
    expect(screen.getByRole('combobox', { name: 'Destination station' })).toHaveValue('MAN');
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
