import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DateInput, DatePickerInput, DateTimePicker } from './LazyDateFields';

// vitest.setup.ts swaps these wrappers for the real fields everywhere else.
vi.unmock('@/components/LazyDateFields');

// The first render imports @mantine/dates for real, which can be slow.
const LOAD = { timeout: 10_000 };

describe('LazyDateFields', () => {
  it('holds the field with a read-only input of the same label, then renders the real picker', async () => {
    renderWithMantine(<DateInput label="Service date" placeholder="Pick a day" description="UK time." />);
    const placeholder = screen.getByLabelText('Service date');
    expect(placeholder).toHaveAttribute('readonly');
    expect(placeholder).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByText('UK time.')).toBeInTheDocument();

    const field = await screen.findByRole('textbox', { name: 'Service date', busy: false }, LOAD);
    expect(field).not.toHaveAttribute('aria-busy');
    expect(field).toHaveAttribute('placeholder', 'Pick a day');
  });

  it('passes the value through to the real picker', async () => {
    renderWithMantine(<DatePickerInput label="From" valueFormat="YYYY-MM-DD" value="2026-10-24" onChange={vi.fn()} />);
    expect(await screen.findByRole('button', { name: 'From' }, LOAD)).toHaveTextContent('2026-10-24');
  });

  it('loads the date-and-time picker too', async () => {
    renderWithMantine(<DateTimePicker label="Scheduled departure" placeholder="Pick date and time" />);
    expect(await screen.findByRole('button', { name: 'Scheduled departure' }, LOAD)).toHaveTextContent(
      'Pick date and time',
    );
  });
});
