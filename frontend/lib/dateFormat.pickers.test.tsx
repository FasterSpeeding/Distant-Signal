import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { DateInput, DatePickerInput, DateTimePicker } from '@mantine/dates';
import { renderWithMantine } from '@/test/render';
import { formatDate, formatDateTime, PICKER_DATE_FORMAT, PICKER_DATE_TIME_FORMAT } from './dateFormat';

// The real `@mantine/dates` fields (several form tests stub them out) with
// the shared formats: they must print what `formatDate`/`formatDateTime`
// print, "17 Sep 2026", not Mantine's "September 17, 2026" or "17/09/2026".
describe('date picker formats', () => {
  it('DateInput shows "17 Sep 2026"', () => {
    renderWithMantine(<DateInput label="Date" valueFormat={PICKER_DATE_FORMAT} value="2026-09-17" />);
    expect(screen.getByLabelText('Date')).toHaveValue('17 Sep 2026');
    expect(formatDate('2026-09-17T12:00:00Z')).toBe('17 Sep 2026');
  });

  it('DatePickerInput shows "17 Sep 2026"', () => {
    renderWithMantine(<DatePickerInput label="Date" valueFormat={PICKER_DATE_FORMAT} value="2026-09-17" />);
    expect(screen.getByRole('button', { name: 'Date' })).toHaveTextContent('17 Sep 2026');
  });

  it('DateTimePicker shows "17 Sep 2026, 08:42"', () => {
    renderWithMantine(
      <DateTimePicker label="Departure" valueFormat={PICKER_DATE_TIME_FORMAT} value="2026-09-17 08:42:00" />,
    );
    expect(screen.getByRole('button', { name: 'Departure' })).toHaveTextContent('17 Sep 2026, 08:42');
    expect(formatDateTime('2026-09-17T07:42:00Z')).toBe('17 Sep 2026, 08:42');
  });
});
