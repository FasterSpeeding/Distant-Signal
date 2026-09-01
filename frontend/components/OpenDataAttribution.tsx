import { Box, Text } from '@mantine/core';

/** Attribution for the third-party open data this app republishes.
 *
 * TfL publishes its Unified API data under a modified Open Government
 * Licence v2.0 whose attribution clause is a condition of use, not a
 * courtesy: "Powered by TfL Open Data" has to appear wherever the data is
 * presented. The wording is fixed — do not paraphrase it.
 *
 * The licence also asks for Ordnance Survey and Geomni attributions where
 * the data used is derived from theirs. That applies to TfL's *geographic*
 * data — StopPoint coordinates, maps, route geometry — and this app ingests
 * none of it: v1 is line status only (see
 * `docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`). If
 * stop-level TfL data is ever added, those two lines have to be added here
 * with it.
 *
 * The four RDM feeds this app consumes are NOT one shared licence family --
 * a Data Sharing Agreement audit (docs/superpowers/plans/2026-09-01-rdm-attribution-wording.md
 * has the full record; the source PDFs no longer exist in this repo) found
 * each agreement's own Schedule 1 Section 8 "ATTRIBUTION" field independently
 * either names a specific required wording or is blank (general "give
 * appropriate credit... in any reasonable manner" clause only). Per feed:
 *   - Darwin Real Time Train Information (Push), the LDBWS/live-departure-
 *     boards source: Schedule 1 requires "powered by NationalRail" verbatim
 *     (lowercase "powered", one word "NationalRail") -- rendered below,
 *     linked to nationalrail.co.uk as a courtesy (not itself required by
 *     Schedule 1's short field, but consistent with linking to the source
 *     where possible).
 *   - NationalRail Knowledgebase Stations (JSON): Schedule 1 requires
 *     "NationalRail (Train Information Services Ltd)" verbatim -- rendered
 *     below as plain text (no link required or added). [NOTE: confirm this
 *     is the actual product this app's Stations subscription is
 *     provisioned under before shipping -- the audit also found a
 *     differently-scoped "Stations Reference Data" product (v1/v1.2) whose
 *     Schedule 1 is blank; see the plan doc above, Task 1, Step 2.]
 *   - Knowledgebase Incidents: Schedule 1 blank; Data Publisher is Rail
 *     Delivery Group, NOT National Rail Enquiries. No line of its own here
 *     -- resting on the general "any reasonable manner" clause, which is a
 *     judgment call this plan's own sign-off task left open, not a settled
 *     conclusion (see the plan doc above, Task 1, Step 1).
 *   - Knowledgebase TOC data: Schedule 1 blank. Same "any reasonable
 *     manner" reasoning as Incidents applies; no line of its own here.
 * Two required strings that both name a National Rail entity are NOT
 * merged/paraphrased into one combined line -- see the plan doc's "Design"
 * section for why: they're independently negotiated Schedule 1 fields with
 * no textual overlap, and this file's own TfL precedent above ("wording is
 * fixed -- do not paraphrase it") already rules out inventing a hybrid
 * string that's verbatim to neither.
 *
 * Network Rail Infrastructure Limited's own open-data feeds (the TRUST
 * movement feed powering individual train tracking) are a THIRD, distinct
 * licence from the NRE terms above -- Network Rail's own terms explicitly
 * prohibit using NR/NRE/TOC branding or describing an app as "official"
 * (see docs/superpowers/specs/2026-08-28-train-tracking-design.md's
 * Licensing section). The line below is deliberately unbranded (no logo,
 * no link styled as an endorsement) and factual rather than using NRE's
 * fixed "Powered by..." wording, which is NRE's own licence condition, not
 * Network Rail's. TODO: this exact wording has not been through the
 * dedicated legal sign-off pass this feature's design doc calls for
 * (separate from the NRE Ts&Cs review below) -- re-verify against Network
 * Rail's current open-data-feeds page before this feature's data ships to
 * real users, the same way the NRE wording above was independently
 * verified first.
 *
 * A plain Server Component with no interactivity, rendered once by the root
 * layout so it is on every page. */
export function OpenDataAttribution() {
  return (
    <Box
      component="footer"
      p="md"
      style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}
    >
      <Text size="xs" c="dimmed">
        Powered by TfL Open Data
      </Text>
      <Text size="xs" c="dimmed">
        <a
          href="https://www.nationalrail.co.uk"
          target="_blank"
          rel="noopener noreferrer"
          style={{ color: 'inherit' }}
        >
          powered by NationalRail
        </a>
      </Text>
      <Text size="xs" c="dimmed">
        NationalRail (Train Information Services Ltd)
      </Text>
      <Text size="xs" c="dimmed">
        Live train movement data from Network Rail&apos;s open data feeds
      </Text>
    </Box>
  );
}
