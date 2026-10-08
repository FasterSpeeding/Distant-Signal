import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import {
  PROVISIONAL_TIMETABLE_MESSAGE,
  PROVISIONAL_TIMETABLE_TITLE,
  ProvisionalTimetableNote,
} from './ProvisionalTimetableNote';

describe('ProvisionalTimetableNote', () => {
  it('says the timetable may change when the response is provisional', () => {
    renderWithMantine(<ProvisionalTimetableNote provisional />);
    expect(screen.getByText(PROVISIONAL_TIMETABLE_TITLE)).toBeInTheDocument();
    expect(screen.getByText(PROVISIONAL_TIMETABLE_MESSAGE)).toBeInTheDocument();
  });

  it.each([false, undefined])('renders nothing when provisional is %s', (provisional) => {
    const { container } = renderWithMantine(<ProvisionalTimetableNote provisional={provisional} />);
    expect(screen.queryByText(PROVISIONAL_TIMETABLE_TITLE)).not.toBeInTheDocument();
    expect(container.querySelector('[data-provisional-timetable]')).toBeNull();
  });
});
