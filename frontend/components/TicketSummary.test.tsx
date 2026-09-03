import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
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

  it('renders the added-on date via formatDateTime', () => {
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
    expect(screen.getByText(/Added/)).toBeInTheDocument();
  });
});
