import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { renderWithMantine } from '@/test/render';
import { trackFeature } from '@/lib/usageAnalytics';
import { resetFinderStops } from '@/lib/transitStopFinder';
import { TransitStopFinder } from './TransitStopFinder';

const push = vi.fn();
vi.mock('next/navigation', () => ({ useRouter: () => ({ push }) }));
// A plain GET form: `next/form` needs the app router.
vi.mock('next/form', () => ({
  default: ({ prefetch: _prefetch, ...props }: ComponentProps<'form'> & { prefetch?: boolean }) => (
    <form method="get" {...props} />
  ),
}));
vi.mock('@/lib/usageAnalytics', () => ({ trackFeature: vi.fn() }));

const LINE_NAMES = { 'metrolink:blue': 'Blue Line', 'metrolink:green': 'Green Line' };

function stopsResponse(names: string[]) {
  return new Response(
    JSON.stringify({
      stops: names.map((name, i) => ({
        id: `9400ZZMA${String(i).padStart(3, '0')}`,
        name,
        lines: ['metrolink:blue', 'metrolink:green'],
      })),
    }),
  );
}

const fetchMock = vi.fn();

function renderFinder(network = 'metrolink') {
  renderWithMantine(<TransitStopFinder network={network} networkName="Manchester Metrolink" lineNames={LINE_NAMES} />);
  return screen.getByRole('combobox', { name: 'Find a stop' });
}

describe('TransitStopFinder', () => {
  beforeEach(() => {
    cleanup();
    resetFinderStops();
    push.mockReset();
    vi.mocked(trackFeature).mockReset();
    fetchMock.mockReset();
    fetchMock.mockResolvedValue(stopsResponse(['Piccadilly', 'Piccadilly Gardens', 'Eccles']));
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it('fetches nothing until it is used', async () => {
    const input = renderFinder();
    expect(fetchMock).not.toHaveBeenCalled();
    fireEvent.focus(input);
    expect(fetchMock).toHaveBeenCalledWith('/api/transit/metrolink/stops');
  });

  it('lists the matches with their lines, says how many, and opens one by keyboard', async () => {
    const input = renderFinder();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'picc' } });
    const options = await screen.findAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual([
      'PiccadillyBlue Line and Green Line',
      'Piccadilly GardensBlue Line and Green Line',
    ]);
    expect(screen.getByRole('status').textContent).toBe('2 stops match.');
    // Arrows move through the rows, announced as the active one.
    fireEvent.keyDown(input, { key: 'ArrowDown', code: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'ArrowDown', code: 'ArrowDown' });
    await waitFor(() => expect(input.getAttribute('aria-activedescendant')).toBe(options[1]!.id));
    fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
    expect(push).toHaveBeenCalledWith('/transit/metrolink/stops/9400ZZMA001');
    expect(trackFeature).toHaveBeenCalledWith('stop_finder', 'stop_opened');
  });

  it('drops the highlighted row when new text filters it away', async () => {
    const input = renderFinder();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'picc' } });
    const options = await screen.findAllByRole('option');
    fireEvent.keyDown(input, { key: 'ArrowDown', code: 'ArrowDown' });
    await waitFor(() => expect(input.getAttribute('aria-activedescendant')).toBe(options[0]!.id));
    fireEvent.change(input, { target: { value: 'zzz' } });
    await screen.findByRole('option', { name: /No stops match/ });
    expect(input.hasAttribute('aria-activedescendant')).toBe(false);
  });

  it('opens the stop named exactly, or the only match, on Enter with no row highlighted', async () => {
    const input = renderFinder();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'piccadilly' } });
    await screen.findAllByRole('option');
    fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
    expect(push).toHaveBeenLastCalledWith('/transit/metrolink/stops/9400ZZMA000');
    fireEvent.change(input, { target: { value: 'eccl' } });
    await screen.findByRole('option', { name: /Eccles/ });
    fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
    expect(push).toHaveBeenLastCalledWith('/transit/metrolink/stops/9400ZZMA002');
  });

  it('leaves Enter on several matches, none highlighted, to the form (the A to Z)', async () => {
    const input = renderFinder();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'picc' } });
    await screen.findAllByRole('option');
    const notPrevented = fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
    expect(notPrevented).toBe(true);
    expect(push).not.toHaveBeenCalled();
  });

  it("opens a TfL network station on TfL's stop page", async () => {
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ stops: [{ id: '9400ZZLUKSX', name: "King's Cross St. Pancras", lines: [] }] })),
    );
    const input = renderFinder('tfl');
    fireEvent.change(input, { target: { value: 'kings cross' } });
    fireEvent.click(await screen.findByRole('option', { name: "King's Cross St. Pancras" }));
    expect(push).toHaveBeenCalledWith('/tfl/stops/940GZZLUKSX');
  });

  it('ends a long list with every match, on the A to Z', async () => {
    fetchMock.mockResolvedValue(stopsResponse(Array.from({ length: 12 }, (_, i) => `Street ${i + 1}`)));
    const input = renderFinder();
    fireEvent.change(input, { target: { value: 'street' } });
    const options = await screen.findAllByRole('option');
    expect(options).toHaveLength(9);
    expect(options[8]!.textContent).toBe('See all 12 matches');
    fireEvent.click(options[8]!);
    expect(push).toHaveBeenCalledWith('/transit/metrolink/stops?q=street');
    expect(trackFeature).toHaveBeenCalledWith('stop_finder', 'all_matches_opened');
  });

  it('says when nothing matches', async () => {
    const input = renderFinder();
    fireEvent.change(input, { target: { value: 'zzz' } });
    expect(await screen.findByRole('option', { name: /No stops match/ })).toBeTruthy();
    expect(screen.getByRole('status').textContent).toBe('No stops match.');
  });

  it('is a search form for the A to Z, for Enter before the list loads or without JavaScript', () => {
    const input = renderFinder();
    const form = input.closest('form')!;
    expect(form.getAttribute('action')).toBe('/transit/metrolink/stops');
    expect(form.getAttribute('role')).toBe('search');
    expect(input.getAttribute('name')).toBe('q');
  });

  it('says when the stops could not be loaded', async () => {
    fetchMock.mockResolvedValue(new Response('', { status: 500 }));
    const input = renderFinder();
    fireEvent.focus(input);
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', expect.stringContaining("Couldn't load"));
  });
});
