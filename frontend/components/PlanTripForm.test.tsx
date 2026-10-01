import { screen, fireEvent, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlanTripForm } from './PlanTripForm';

// `renderWithMantine`, not a bare `render` -- every field here is a
// `@mantine/core`/`@mantine/dates` component, which throws ("MantineProvider
// was not found in component tree") without a real `MantineProvider`
// ancestor. Same convention every other Mantine-backed form test in this
// repo already uses (e.g. `TrackTrainForm.test.tsx`), rather than this
// file's own hand-rolled provider.
// A fixed "now" for the "defaults to now, not midnight" tests below --
// `departAfter` now defaults to `nowInLondon().format('HH:mm')` at mount (this
// task's own "default to now" fix), so asserting against it needs the real
// wall-clock time pinned to something known, exactly the same reasoning
// `TrackTrainForm.test.tsx`'s own `FIXED_NOW` gives for its
// `scheduledDeparture` default. `shouldAdvanceTime` (same option
// `TrackTrainForm.test.tsx`/`AutoRefresh.test.tsx` already use) lets real
// `setTimeout`-driven async machinery keep working normally while `Date`
// itself stays pinned near this fixed point.
const FIXED_NOW = '2026-09-05T14:32:00.000Z';

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
    fireEvent.change(screen.getAllByPlaceholderText('Station name or CRS code')[2]!, { target: { value: 'YRK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ originCrs: 'EUS', destinationCrs: 'EDB', waypointCrs: ['YRK'], results: 'fastest' }),
    );
  });

  // Regression coverage for the "defaults to start of day, not now" bug:
  // `departAfter` used to default to `''`, which `buildTripPlanQuery`
  // (`lib/tripPlan.ts`) then omitted from the query string entirely, and
  // `GET /Trips/plan` (`crates/api/src/routes/trips.rs`) defaults an absent
  // `departAfter` to `NaiveTime::MIN` -- so a visitor who searched without
  // touching this field silently got an itinerary search from midnight.
  describe('departAfter defaults to now, not midnight', () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      vi.setSystemTime(new Date(FIXED_NOW));
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('pre-fills the Depart after field with the current local time on mount', () => {
      renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
      expect(screen.getByLabelText('Depart after (optional)')).toHaveValue('15:32'); // London BST wall clock (FE-4);
    });

    it('submits the current time as departAfter when the visitor never touches the field', () => {
      const onSubmit = vi.fn();
      renderWithMantine(<PlanTripForm onSubmit={onSubmit} />);
      fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
      fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
      fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
      expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ departAfter: '15:32' }));
    });

    it('defaults date and time to London wall clock, not the host zone (FE-4)', () => {
      // 23:30 UTC on 15 July is 00:30 on 16 July in London (BST).
      vi.setSystemTime(new Date('2026-07-15T23:30:00Z'));
      const onSubmit = vi.fn();
      renderWithMantine(<PlanTripForm onSubmit={onSubmit} />);
      fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
      fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
      fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
      expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ date: '2026-07-16', departAfter: '00:30' }));
    });

    it('still lets a visitor clear the field back to no lower bound at all', () => {
      const onSubmit = vi.fn();
      renderWithMantine(<PlanTripForm onSubmit={onSubmit} />);
      fireEvent.change(screen.getByRole('combobox', { name: 'From' }), { target: { value: 'EUS' } });
      fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'EDB' } });
      fireEvent.change(screen.getByLabelText('Depart after (optional)'), { target: { value: '' } });
      fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));
      expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ departAfter: undefined }));
    });
  });

  // The date picker's minDate was `new Date()`, the browser's local day. A
  // visitor ahead of UK time near midnight is already on tomorrow, so
  // London's today -- the very day `date` defaults to -- was disabled.
  describe("the date picker's earliest day is London's today, not the browser's", () => {
    const originalTz = process.env.TZ;
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });
    afterEach(() => {
      vi.useRealTimers();
      if (originalTz === undefined) {
        delete process.env.TZ;
      } else {
        process.env.TZ = originalTz;
      }
    });

    // By aria-label, not role: the dropdown is still mid-transition (not yet
    // "visible" to a role query) when its day buttons are rendered.
    function dayButton(label: string): Promise<HTMLElement> {
      return waitFor(() => {
        const day = document.querySelector<HTMLElement>(`[aria-label="${label}"]`);
        if (!day) throw new Error(`no day ${label}`);
        return day;
      });
    }

    it("lets a visitor ahead of UK time (Tokyo) pick London's today", async () => {
      process.env.TZ = 'Asia/Tokyo'; // UTC+9
      // 16:30Z: 17:30 on 5 Sep in London, already 01:30 on 6 Sep in Tokyo.
      vi.setSystemTime(new Date('2026-09-05T16:30:00.000Z'));
      renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
      fireEvent.focus(screen.getByLabelText('Date'));
      expect(await dayButton('5 September 2026')).not.toBeDisabled();
      expect(await dayButton('4 September 2026')).toBeDisabled();
    });

    it("does not offer London's yesterday to a visitor behind UK time", async () => {
      process.env.TZ = 'Etc/GMT+2'; // POSIX sign convention: this is UTC-2
      // 23:30Z: 00:30 on 6 Sep in London, still 21:30 on 5 Sep at UTC-2.
      vi.setSystemTime(new Date('2026-09-05T23:30:00.000Z'));
      renderWithMantine(<PlanTripForm onSubmit={vi.fn()} />);
      fireEvent.focus(screen.getByLabelText('Date'));
      expect(await dayButton('6 September 2026')).not.toBeDisabled();
      expect(await dayButton('5 September 2026')).toBeDisabled();
    });
  });
});
