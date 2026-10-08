import { Alert, Text } from '@mantine/core';

/** The copy, exported so the pages' tests can assert on it without
 * duplicating it. */
export const PROVISIONAL_TIMETABLE_TITLE = 'Timetable may change';
export const PROVISIONAL_TIMETABLE_MESSAGE =
  'This date is far enough ahead that its timetable is provisional: engineering works and other late changes are often added in the last few weeks before the day. Check again closer to the time.';

/** "Timetable may change", shown wherever a response for a far-ahead
 * service date says `provisional: true` (`/public/trains/search`,
 * `GET /Train/by-uid/{uid}/{date}`, `GET /Trips/plan`; see
 * `crates/api/src/routes/provisional.rs` for the rule and
 * docs/api-changelog.md for the contract). Renders nothing otherwise,
 * including when the field is absent (an older backend). */
export function ProvisionalTimetableNote({ provisional }: { provisional?: boolean | undefined }) {
  if (provisional !== true) return null;
  return (
    <Alert color="yellow" title={PROVISIONAL_TIMETABLE_TITLE} data-provisional-timetable>
      <Text size="sm">{PROVISIONAL_TIMETABLE_MESSAGE}</Text>
    </Alert>
  );
}
