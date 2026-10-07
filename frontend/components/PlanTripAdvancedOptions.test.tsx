import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlanTripForm } from './PlanTripForm';

// Plain async functions, not `vi.fn()` (see `PlanTripFlow.test.tsx`'s C2
// note). The via picker searches stations only; the avoid pickers the
// planner search, which also returns bus stops.
vi.mock('@/lib/suggestions', () => ({
  searchStations: async () => [{ code: 'STA', name: 'Stafford' }],
  searchPlannerLocations: async () => [
    { code: 'STA', name: 'Stafford' },
    { code: 'tiploc:STAFBUS', name: 'Stafford Bus Station (bus)' },
  ],
  searchTocs: async () => [],
  getStationNames: async (codes: string[]) =>
    new Map(codes.filter((code) => code === 'CRE').map((code) => [code, 'Crewe'] as [string, string])),
}));

// The panel opens through a transition, so its fields appear a tick later
// (as in `WorkingTimetable.test.tsx`).
async function openAdvanced() {
  fireEvent.click(screen.getByRole('button', { name: /Advanced options/ }));
  await screen.findByRole('combobox', { name: 'Pass through (in order)' });
}

function addTyped(label: string, code: string) {
  const input = screen.getByRole('combobox', { name: label });
  fireEvent.change(input, { target: { value: code } });
  fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
}

function fillEnds(origin = 'EUS', destination = 'GLA') {
  fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: origin } });
  fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: destination } });
}

describe('PlanTripForm advanced options', () => {
  it('is collapsed by default, with no options set', () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    const control = screen.getByRole('button', { name: 'Advanced options' });
    expect(control).toHaveAttribute('aria-expanded', 'false');
  });

  it('shows the fields once expanded, and explains via vs call at', async () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    await openAdvanced();
    expect(screen.getByRole('button', { name: /Advanced options/ })).toHaveAttribute('aria-expanded', 'true');
    for (const label of ['Pass through (in order)', 'Avoid completely', "Don't stop at", "Don't change at"]) {
      expect(screen.getByRole('combobox', { name: label })).toBeInTheDocument();
    }
    expect(screen.getByLabelText(/Most changes/)).toHaveValue('');
    expect(screen.getByText(/whether or not the train stops there/)).toBeInTheDocument();
    expect(screen.getByText(/Stations only, not bus stops/)).toBeInTheDocument();
  });

  it('says how many options are set while collapsed, and which', async () => {
    renderWithMantine(
      <PlanTripForm onSubmit={vi.fn()} initial={{ viaCrs: ['CRE'], avoidCrs: ['BHM'], maxChanges: 1 }} />,
    );
    expect(screen.getByRole('button', { name: 'Advanced options (3 set)' })).toHaveAttribute('aria-expanded', 'false');
    // Restored codes get their names looked up.
    expect(await screen.findByText('Pass through Crewe · Avoid BHM · Max 1 change')).toBeInTheDocument();
  });

  it('adds, reorders and removes vias, with labelled buttons', async () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    await openAdvanced();
    addTyped('Pass through (in order)', 'sta');
    addTyped('Pass through (in order)', 'CRE');
    const list = screen.getByRole('list', { name: 'Pass through (in order)' });
    expect(
      within(list)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['1.STA', '2.CRE']);
    // The typed field is cleared for the next station.
    expect(screen.getByRole('combobox', { name: 'Pass through (in order)' })).toHaveValue('');

    expect(screen.getByRole('button', { name: 'Move STA up' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Move CRE up' }));
    expect(
      within(list)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['1.CRE', '2.STA']);
    // Focus follows the moved row (it reached the top, so its other button).
    await waitFor(() => expect(screen.getByRole('button', { name: 'Move CRE down' })).toHaveFocus());
    expect(screen.getByText('Moved CRE to position 1 of 2.')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Remove CRE from pass-through stations' }));
    expect(
      within(list)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['1.STA']);
  });

  it('stops at 3 vias', async () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    await openAdvanced();
    for (const code of ['STA', 'CRE', 'RUG']) addTyped('Pass through (in order)', code);
    expect(screen.getByRole('combobox', { name: 'Pass through (in order)' })).toBeDisabled();
    expect(screen.getByText(/Up to 3: remove one to add another/)).toBeInTheDocument();
  });

  it('refuses a typed bus stop as a via, but accepts one to avoid', async () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    await openAdvanced();
    addTyped('Pass through (in order)', 'tiploc:STAFBUS');
    const via = screen.getByRole('combobox', { name: 'Pass through (in order)' });
    expect(via).toHaveAttribute('aria-invalid', 'true');
    expect(screen.queryByRole('list', { name: 'Pass through (in order)' })).not.toBeInTheDocument();
    addTyped("Don't stop at", 'tiploc:stafbus');
    expect(within(screen.getByRole('list', { name: "Don't stop at" })).getByText('tiploc:STAFBUS')).toBeInTheDocument();
  });

  it('offers no bus stops in the via suggestions', async () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    await openAdvanced();
    fireEvent.change(screen.getByRole('combobox', { name: 'Pass through (in order)' }), { target: { value: 'staf' } });
    expect(await screen.findByText('STA — Stafford')).toBeInTheDocument();
    expect(screen.queryByText('Stafford Bus Station (bus)')).not.toBeInTheDocument();
  });

  it('sends every advanced option with the search', async () => {
    const onSubmit = vi.fn();
    renderWithMantine(<PlanTripForm onSubmit={onSubmit} />);
    fillEnds();
    await openAdvanced();
    addTyped('Pass through (in order)', 'STA');
    addTyped('Avoid completely', 'BHM');
    addTyped("Don't stop at", 'CRE');
    addTyped("Don't change at", 'WVH');
    fireEvent.change(screen.getByLabelText(/Most changes/), { target: { value: '0' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        viaCrs: ['STA'],
        avoidCrs: ['BHM'],
        avoidStopCrs: ['CRE'],
        avoidChangeCrs: ['WVH'],
        maxChanges: 0,
      }),
    );
  });

  it('sends no advanced options by default', () => {
    const onSubmit = vi.fn();
    renderWithMantine(<PlanTripForm onSubmit={onSubmit} />);
    fillEnds();
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
    const query = onSubmit.mock.calls[0]![0] as Record<string, unknown>;
    for (const key of ['viaCrs', 'avoidCrs', 'avoidStopCrs', 'avoidChangeCrs', 'maxChanges']) {
      expect(query).not.toHaveProperty(key);
    }
  });

  it('flags a via that is the origin on the field and by the button, and opens the section on submit', async () => {
    const onSubmit = vi.fn();
    renderWithMantine(<PlanTripForm onSubmit={onSubmit} initial={{ viaCrs: ['EUS'] }} />);
    fillEnds('EUS');
    const problem = screen.getByText(/Check Advanced options: EUS is where you start/);
    const submit = screen.getByRole('button', { name: 'Find routes' });
    expect(submit).toHaveAttribute('aria-describedby', problem.id);
    expect(screen.getByRole('button', { name: 'Advanced options (1 set, 1 to fix)' })).toBeInTheDocument();
    fireEvent.click(submit);
    expect(onSubmit).not.toHaveBeenCalled();
    const via = await screen.findByRole('combobox', { name: 'Pass through (in order)' });
    expect(via).toHaveAttribute('aria-invalid', 'true');
    expect(via).toHaveAccessibleDescription(expect.stringContaining('every journey passes it already'));
  });

  it('flags a via that is also avoided, and an avoided destination', async () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} initial={{ viaCrs: ['STA'], avoidCrs: ['STA', 'GLA'] }} />);
    fillEnds();
    await openAdvanced();
    expect(screen.getByRole('combobox', { name: 'Pass through (in order)' })).toHaveAccessibleDescription(
      expect.stringContaining('also in "Avoid completely"'),
    );
    expect(screen.getByRole('combobox', { name: 'Avoid completely' })).toHaveAccessibleDescription(
      expect.stringContaining("GLA is where you finish, so it can't be avoided"),
    );
  });

  it('restores the whole form from initial values', () => {
    renderWithMantine(
      <PlanTripForm
        onSubmit={vi.fn()}
        initial={{
          originCrs: 'EUS',
          destinationCrs: 'GLA',
          waypointCrs: ['PRE'],
          departAfter: '09:15',
          results: 'options',
        }}
      />,
    );
    expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('EUS');
    expect(screen.getByRole('combobox', { name: 'To' })).toHaveValue('GLA');
    expect(screen.getByLabelText(/Call at/)).toHaveValue('PRE');
    expect(screen.getByLabelText('Depart after (optional)')).toHaveValue('09:15');
    expect(screen.getByRole('radio', { name: 'Compare options' })).toBeChecked();
  });
});
