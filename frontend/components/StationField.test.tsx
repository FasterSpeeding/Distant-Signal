import { useState } from 'react';
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { resetCodeLabels, typedPlannerCode } from '@/lib/codeLabels';
import { resetStationGroupsCache } from '@/lib/stationGroups';
import type { Suggestion } from '@/lib/types';
import { StationField } from './StationField';
import { OperatorField } from './OperatorField';

const EUSTON: Suggestion = { code: 'EUS', name: 'London Euston' };
const EUSTON_SQUARE: Suggestion = { code: 'EUSQ', name: 'Euston Square' };

/** A parent holding the code, as every form does, showing what it holds. */
function Harness({
  initial = '',
  search,
  allowTypedCode,
  onValue,
}: {
  initial?: string;
  search?: (q: string, signal: AbortSignal) => Promise<Suggestion[]>;
  allowTypedCode?: boolean | ((text: string) => string | null);
  onValue?: (value: string) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <StationField
        label="From"
        value={value}
        onChange={(next) => {
          setValue(next);
          onValue?.(next);
        }}
        {...(search !== undefined && { search })}
        {...(allowTypedCode !== undefined && { allowTypedCode })}
      />
      <span data-testid="held">{value}</span>
    </>
  );
}

function field() {
  return screen.getByRole('combobox', { name: 'From' });
}

function held() {
  return screen.getByTestId('held').textContent;
}

describe('StationField', () => {
  beforeEach(() => {
    resetCodeLabels();
    resetStationGroupsCache();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows the picked station by name and hands the parent its code', async () => {
    const search = vi.fn(async () => [EUSTON, EUSTON_SQUARE]);
    renderWithMantine(<Harness search={search} />);
    fireEvent.change(field(), { target: { value: 'eust' } });
    expect(held()).toBe('eust');
    fireEvent.click(await screen.findByRole('option', { name: 'EUS · London Euston' }));
    expect(field()).toHaveValue('London Euston (EUS)');
    expect(held()).toBe('EUS');
    // Nothing more is searched while the label stands.
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith('eust', expect.anything());
  });

  it('announces the pick politely', async () => {
    renderWithMantine(<Harness search={async () => [EUSTON]} />);
    const status = screen.getByRole('status');
    expect(status).toHaveAttribute('aria-live', 'polite');
    expect(status).toHaveTextContent('');
    fireEvent.change(field(), { target: { value: 'eus' } });
    fireEvent.click(await screen.findByRole('option', { name: 'EUS · London Euston' }));
    expect(status).toHaveTextContent('From set to London Euston (EUS)');
  });

  it('picks a typed code on blur, named from the suggestions', async () => {
    renderWithMantine(<Harness search={async () => [EUSTON, EUSTON_SQUARE]} />);
    fireEvent.change(field(), { target: { value: 'eus' } });
    await screen.findByRole('option', { name: 'EUS · London Euston' });
    fireEvent.blur(field());
    expect(field()).toHaveValue('London Euston (EUS)');
    expect(held()).toBe('EUS');
  });

  it('picks a typed code on Enter', async () => {
    renderWithMantine(<Harness search={async () => [EUSTON]} />);
    fireEvent.change(field(), { target: { value: 'EUS' } });
    await screen.findByRole('option', { name: 'EUS · London Euston' });
    fireEvent.keyDown(field(), { key: 'Enter', code: 'Enter' });
    expect(field()).toHaveValue('London Euston (EUS)');
    expect(held()).toBe('EUS');
  });

  it('picks a typed station name on blur', async () => {
    renderWithMantine(<Harness search={async () => [EUSTON]} />);
    fireEvent.change(field(), { target: { value: 'london euston' } });
    await screen.findByRole('option', { name: 'EUS · London Euston' });
    fireEvent.blur(field());
    expect(held()).toBe('EUS');
  });

  it('shows an unknown typed code bare', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('[]', { status: 200 })),
    );
    renderWithMantine(<Harness search={async () => []} />);
    fireEvent.change(field(), { target: { value: 'xyz' } });
    fireEvent.blur(field());
    expect(held()).toBe('XYZ');
    expect(field()).toHaveValue('XYZ');
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(field()).toHaveValue('XYZ');
  });

  it('leaves free text typed when nothing matches it', () => {
    renderWithMantine(<Harness search={async () => []} />);
    fireEvent.change(field(), { target: { value: 'Somewhere' } });
    fireEvent.blur(field());
    expect(field()).toHaveValue('Somewhere');
    expect(held()).toBe('Somewhere');
    expect(screen.queryByRole('button', { name: 'Clear From' })).not.toBeInTheDocument();
  });

  it('drops the pick on any edit and searches the new text', async () => {
    const search = vi.fn(async () => [EUSTON]);
    renderWithMantine(<Harness search={search} />);
    fireEvent.change(field(), { target: { value: 'eus' } });
    fireEvent.click(await screen.findByRole('option', { name: 'EUS · London Euston' }));
    expect(held()).toBe('EUS');
    fireEvent.change(field(), { target: { value: 'London Euston (EU' } });
    expect(held()).toBe('London Euston (EU');
    expect(screen.queryByRole('button', { name: 'Clear From' })).not.toBeInTheDocument();
    await waitFor(() => expect(search).toHaveBeenLastCalledWith('London Euston (EU', expect.anything()));
  });

  it('selects the whole label on focus', async () => {
    renderWithMantine(<Harness search={async () => [EUSTON]} />);
    fireEvent.change(field(), { target: { value: 'eus' } });
    fireEvent.click(await screen.findByRole('option', { name: 'EUS · London Euston' }));
    const input = field() as HTMLInputElement;
    input.setSelectionRange(0, 0);
    fireEvent.focus(input);
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe('London Euston (EUS)'.length);
  });

  it('names a deep-linked code once it resolves', async () => {
    const fetchMock = vi.fn(async (url: string) =>
      url.startsWith('/api/stations?q=EUS')
        ? new Response(JSON.stringify([EUSTON, EUSTON_SQUARE]), { status: 200 })
        : new Response('[]', { status: 404 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<Harness initial="EUS" />);
    expect(field()).toHaveValue('EUS');
    await waitFor(() => expect(field()).toHaveValue('London Euston (EUS)'));
    expect(held()).toBe('EUS');
    // Looked up once, however many fields show it.
    renderWithMantine(<StationField label="To" value="EUS" onChange={vi.fn()} />);
    expect(screen.getByRole('combobox', { name: 'To' })).toHaveValue('London Euston (EUS)');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('uses a label the page already knows without looking it up', () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationField label="From" value="EUS" knownLabel="London Euston (EUS)" onChange={vi.fn()} />);
    expect(field()).toHaveValue('London Euston (EUS)');
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('names a deep-linked TfL stop by its name and modes, never its id', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        url === '/api/tfl/stops/940GZZLUODS'
          ? new Response(
              JSON.stringify({ id: '940GZZLUODS', name: 'Old Street', modes: ['tube'], lines: [], railStations: [] }),
              { status: 200 },
            )
          : new Response('[]', { status: 404 }),
      ),
    );
    renderWithMantine(<Harness initial="940GZZLUODS" />);
    await waitFor(() => expect(field()).toHaveValue('Old Street (Tube)'));
    expect(held()).toBe('940GZZLUODS');
  });

  it('names a deep-linked station group by its label', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        url === '/api/Trips/station-groups'
          ? new Response(
              JSON.stringify({
                groups: [
                  {
                    group: 'LON',
                    code: 'group:LON',
                    name: 'London Terminals',
                    members: [
                      { crs: 'EUS', name: 'London Euston' },
                      { crs: 'KGX', name: 'London Kings Cross' },
                    ],
                  },
                ],
              }),
              { status: 200 },
            )
          : new Response('[]', { status: 404 }),
      ),
    );
    renderWithMantine(<Harness initial="group:LON" />);
    await waitFor(() => expect(field()).toHaveValue('Any of the London Terminals (2 stations)'));
  });

  it('picks a typed planner code with the planner parser', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('[]', { status: 404 })),
    );
    renderWithMantine(
      <Harness search={async () => []} allowTypedCode={(text) => typedPlannerCode(text, { tflStops: true })} />,
    );
    fireEvent.change(field(), { target: { value: 'group:lon' } });
    fireEvent.blur(field());
    expect(held()).toBe('group:LON');
  });

  it('only picks listed suggestions without allowTypedCode', () => {
    renderWithMantine(<Harness search={async () => []} allowTypedCode={false} />);
    fireEvent.change(field(), { target: { value: 'eus' } });
    fireEvent.blur(field());
    expect(held()).toBe('eus');
    expect(field()).toHaveValue('eus');
  });

  it('clears with the clear button, returning focus to the input', async () => {
    renderWithMantine(<Harness search={async () => [EUSTON]} />);
    expect(screen.queryByRole('button', { name: 'Clear From' })).not.toBeInTheDocument();
    fireEvent.change(field(), { target: { value: 'eus' } });
    fireEvent.click(await screen.findByRole('option', { name: 'EUS · London Euston' }));
    const clear = screen.getByRole('button', { name: 'Clear From' });
    act(() => clear.focus());
    fireEvent.click(clear);
    expect(field()).toHaveValue('');
    expect(held()).toBe('');
    expect(field()).toHaveFocus();
    expect(screen.getByRole('status')).toHaveTextContent('From cleared');
    expect(screen.queryByRole('button', { name: 'Clear From' })).not.toBeInTheDocument();
  });

  it('follows a code the parent sets', async () => {
    function Swapper() {
      const [value, setValue] = useState('EUS');
      return (
        <>
          <StationField label="From" value={value} onChange={setValue} knownLabel="London Euston (EUS)" />
          <button type="button" onClick={() => setValue('')}>
            Reset
          </button>
        </>
      );
    }
    vi.stubGlobal('fetch', vi.fn());
    renderWithMantine(<Swapper />);
    expect(field()).toHaveValue('London Euston (EUS)');
    fireEvent.click(screen.getByRole('button', { name: 'Reset' }));
    expect(field()).toHaveValue('');
  });

  it('shows the inert no-match row once a search comes back empty', async () => {
    renderWithMantine(<Harness search={async () => []} />);
    fireEvent.change(field(), { target: { value: 'zzz' } });
    const row = await screen.findByRole('option', { name: 'No matching stations' });
    fireEvent.click(row);
    expect(held()).toBe('zzz');
  });
});

describe('OperatorField', () => {
  beforeEach(() => {
    resetCodeLabels();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows the operator by name with its code', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([{ code: 'VT', name: 'Avanti West Coast' }]), { status: 200 })),
    );
    const onChange = vi.fn();
    function Parent() {
      const [value, setValue] = useState('');
      return (
        <OperatorField
          label="Operator"
          value={value}
          onChange={(next) => {
            setValue(next);
            onChange(next);
          }}
        />
      );
    }
    renderWithMantine(<Parent />);
    fireEvent.change(screen.getByRole('combobox', { name: 'Operator' }), { target: { value: 'avanti' } });
    fireEvent.click(await screen.findByRole('option', { name: 'VT · Avanti West Coast' }));
    expect(onChange).toHaveBeenLastCalledWith('VT');
    expect(screen.getByRole('combobox', { name: 'Operator' })).toHaveValue('Avanti West Coast (VT)');
  });
});
