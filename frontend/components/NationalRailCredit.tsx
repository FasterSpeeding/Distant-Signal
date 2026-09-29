import { Text } from '@mantine/core';
import { TextLink } from './TextLink';

export const NATIONAL_RAIL_URL = 'https://www.nationalrail.co.uk';

/** LEG-23: the National Rail credit, placed directly under a block whose
 * data comes predominantly from Darwin/LDBWS (the live departure boards).
 * NRE's developer guidelines ask for the credit "alongside" such data, not
 * only in the site-wide footer.
 *
 * The wording is the footer's: the Darwin Schedule 1 string "powered by
 * NationalRail" (linked) followed by the Knowledgebase Stations string
 * "NationalRail (Train Information Services Ltd)". See
 * `OpenDataAttribution.tsx` for why the two are joined. Text only: the NRE
 * logo needs National Rail's written permission.
 *
 * Operating basis: on 2026-09-27 the operator decided to treat the current
 * and planned use as permitted under the Rail Data Marketplace terms, so
 * this wording is final rather than pending Schedule 1 review.
 *
 * Kept free of server-only imports so client components (the `/track`
 * live-departures picker) can render it too. */
export function NationalRailCredit() {
  return (
    <Text size="xs" c="dimmed" data-nre-credit>
      Live departure data{' '}
      <TextLink
        href={NATIONAL_RAIL_URL}
        target="_blank"
        rel="noopener noreferrer"
        underline="always"
        inline
        tone="inherit"
      >
        powered by NationalRail
      </TextLink>
      {' (Train Information Services Ltd)'}
    </Text>
  );
}
