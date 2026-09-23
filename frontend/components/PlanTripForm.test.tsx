import { screen, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlanTripForm } from './PlanTripForm';

// `renderWithMantine`, not a bare `render` -- every field here is a
// `@mantine/core`/`@mantine/dates` component, which throws ("MantineProvider
// was not found in component tree") without a real `MantineProvider`
// ancestor. Same convention every other Mantine-backed form test in this
// repo already uses (e.g. `TrackTrainForm.test.tsx`), rather than this
// file's own hand-rolled provider.
describe('PlanTripForm', () => {
  it('disables Find routes until both origin and destination are entered', () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    // `getByRole('button', { name })`, not `getByText` -- Mantine's
    // `Button` wraps its label in an inner `<span
    // class="mantine-Button-label">`, so `getByText('Find routes')`
    // resolves to that span, not the `<button>` itself; `toBeDisabled`
    // only recognises the disabled state on an actual form control
    // (button/input/etc.), so asserting against the span always reads as
    // "not disabled" regardless of the real button's state. Same
    // `getByRole('button', { name })` convention every other disabled-
    // submit-button test in this repo already uses (e.g.
    // `CreateGroupForm.test.tsx`, `AddJourneyLegButton.test.tsx`).
    const submit = screen.getByRole('button', { name: 'Find routes' });
    expect(submit).toBeDisabled();
    // `getByRole('combobox', { name })`, not `getByLabelText` -- the real
    // `Autocomplete` wiring (this task's own Step 1) renders an
    // `aria-labelledby`'d options listbox `<div>` alongside the `<input>`,
    // and both resolve to the same accessible name, so `getByLabelText`
    // matches two elements once real suggestion data is wired in. Same
    // `getByRole('combobox', { name })` convention
    // `TrackTrainForm.test.tsx`'s own Origin-Autocomplete tests already
    // use.
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    expect(submit).not.toBeDisabled();
  });

  it('adds and removes waypoint fields', () => {
    renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
    // `getAllByPlaceholderText`, not `queryByPlaceholderText` -- From and
    // To already share this placeholder before any waypoint exists (this
    // component's own Step 2 code), so the singular query throws "found
    // multiple elements" even pre-waypoint; the assertion's real intent
    // (this placeholder exists at all) survives as a non-empty list check.
    // No `{ selector: 'input' }` option -- `getByPlaceholderText`'s
    // `MatcherOptions` doesn't accept one (that's `getByText`-only; `tsc`
    // rejects it here), and every match is already an `<input>` anyway.
    expect(screen.getAllByPlaceholderText('Station name or CRS code').length).toBeGreaterThan(0);
    fireEvent.click(screen.getByText('Add a waypoint'));
    const waypointInputs = screen.getAllByPlaceholderText('Station name or CRS code');
    // From + To + 1 waypoint = 3 inputs sharing this placeholder.
    expect(waypointInputs.length).toBe(3);
    fireEvent.click(screen.getByLabelText('Remove this waypoint'));
    expect(screen.getAllByPlaceholderText('Station name or CRS code').length).toBe(2);
  });

  it('calls onSubmit with a well-formed query, including entered-order waypoints', () => {
    const onSubmit = vi.fn();
    renderWithMantine(<PlanTripForm onSubmit={onSubmit} />);
    fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
    fireEvent.click(screen.getByText('Add a waypoint'));
    fireEvent.change(screen.getAllByPlaceholderText('Station name or CRS code')[2], { target: { value: 'YRK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ originCrs: 'EUS', destinationCrs: 'EDB', waypointCrs: ['YRK'], results: 'fastest' })
    );
  });
});
