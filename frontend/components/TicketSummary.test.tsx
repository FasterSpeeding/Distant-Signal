import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { formatLocalDateTime } from '@/lib/dateFormat';
import { TicketSummary } from './TicketSummary';

describe('TicketSummary', () => {
  it('renders operator and ticket type', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: 'Off-Peak Day Single',
          originCrs: null,
          destinationCrs: null,
          originName: null,
          destinationName: null,
          source: 'manual',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText('LNER — Off-Peak Day Single')).toBeInTheDocument();
  });

  it('falls back to "Ticket" when operator is null', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: null,
          ticketType: null,
          originCrs: null,
          destinationCrs: null,
          originName: null,
          destinationName: null,
          source: 'manual',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText('Ticket')).toBeInTheDocument();
  });

  it('renders the route when either origin or destination is present', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: 'KGX',
          destinationCrs: 'EDB',
          originName: null,
          destinationName: null,
          source: 'manual',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText('KGX → EDB')).toBeInTheDocument();
  });

  it('renders station names in the route when the backend resolved them', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: 'KGX',
          destinationCrs: 'EDB',
          originName: 'London Kings Cross',
          destinationName: 'Edinburgh Waverley',
          source: 'manual',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText('London Kings Cross (KGX) → Edinburgh Waverley (EDB)')).toBeInTheDocument();
  });

  it('renders no route line when both origin and destination are null', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: null,
          destinationCrs: null,
          originName: null,
          destinationName: null,
          source: 'manual',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.queryByText(/→/)).not.toBeInTheDocument();
  });

  it('renders a provenance badge for the ticket source', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: null,
          destinationCrs: null,
          originName: null,
          destinationName: null,
          source: 'pkpass-semantics',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText('From Wallet pass')).toBeInTheDocument();
  });

  it('renders the added-on date in the viewer\'s own timezone, not London', () => {
    // "Added" is the app's one viewer-relative timestamp (TicketSummary.tsx's
    // comment at the swap site), so post-mount it renders in whatever zone
    // this test process is in -- expected via `formatLocalDateTime` rather
    // than a literal, which would make the assertion machine-dependent.
    // `renderWithMantine` mounts, so the `useMounted()` gate inside
    // `LocalDateTime` has already flipped by the time this asserts; the
    // pre-mount London fallback is covered in LocalDateTime.test.tsx.
    const createdAt = '2026-08-29T12:00:00Z';
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: null,
          destinationCrs: null,
          originName: null,
          destinationName: null,
          source: 'manual',
          createdAt,
        }}
      />,
    );
    expect(screen.getByText(`Added ${formatLocalDateTime(createdAt)}`)).toBeInTheDocument();
  });
});
