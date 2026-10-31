import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { buildLineDiagram } from '@/lib/transitLineDiagram';
import type { TransitStationRef } from '@/lib/types';
import { TransitLineDiagram } from './TransitLineDiagram';

const st = (id: string, name: string): TransitStationRef => ({ id, name });
const MDN = st('9400ZZLUMDN', 'Morden');
const KEN = st('9400ZZLUKEN', 'Kennington');
const EGW = st('9400ZZLUEGW', 'Edgware');
const BPS = st('9400ZZLUBPS', 'Battersea Power Station');
const HBT = st('9400ZZLUHBT', 'High Barnet');
const RARE = st('9400ZZLURAR', 'Rare Stop');

function render() {
  const diagram = buildLineDiagram({
    directions: [{ direction: 'outbound', headsign: null, stations: [MDN, KEN, EGW, BPS, HBT, RARE] }],
    branches: [
      { id: 'outbound-1', direction: 'outbound', name: 'a', stations: [MDN, KEN, EGW], sharePercent: 60 },
      { id: 'outbound-2', direction: 'outbound', name: 'b', stations: [BPS, KEN, HBT], sharePercent: 40 },
    ],
  });
  renderWithMantine(
    <TransitLineDiagram network="tfl" diagram={diagram} colour="#222" label="Northern stops" idPrefix="d" />,
  );
}

describe('TransitLineDiagram', () => {
  it('lists the main run in order, each stop a link, its ends marked', () => {
    render();
    const list = screen.getByRole('list', { name: 'Northern stops' });
    const items = within(list)
      .getAllByRole('listitem')
      .filter((li) => li.parentElement === list);
    expect(items.map((li) => li.querySelector('a')?.textContent)).toEqual(['Morden', 'Kennington', 'Edgware']);
    expect(items.map((li) => li.hasAttribute('data-terminus'))).toEqual([true, false, true]);
    expect(within(list).getByRole('link', { name: 'Morden' }).getAttribute('href')).toBe('/tfl/stops/940GZZLUMDN');
  });

  it('nests each branch, named by its label, in the stop where it splits or joins', () => {
    render();
    const kennington = screen.getByRole('link', { name: 'Kennington' }).closest('li')!;
    const from = within(kennington).getByRole('group', {
      name: 'Branch from Battersea Power Station, joining at Kennington',
    });
    expect(within(from).getByRole('link', { name: 'Battersea Power Station' })).toBeTruthy();
    const to = within(kennington).getByRole('group', { name: 'Branch to High Barnet' });
    expect(within(to).getAllByRole('listitem')).toHaveLength(1);
  });

  it('names stations too few trips call at after the strip', () => {
    render();
    expect(screen.getByText(/Some trips also call at/).textContent).toBe('Some trips also call at Rare Stop.');
  });
});
