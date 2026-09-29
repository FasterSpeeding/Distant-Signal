import { Box, Group, List, ListItem, Stack, Text } from '@mantine/core';
import { SectionTitle } from './SectionTitle';
import type { ReactNode } from 'react';
import { TextLink } from './TextLink';
import { LEGAL_LINKS, legalPagesVisible } from '@/lib/legal';
import { NATIONAL_RAIL_URL } from './NationalRailCredit';

// Re-exported so existing imports keep working; it lives in its own module
// because client components render it too.
export { NationalRailCredit } from './NationalRailCredit';

/** Attribution for the third-party open data this app republishes: the
 * short site footer (`OpenDataAttribution`, on every page) and the full
 * `/attribution` page (`OpenDataAttributionDetails`). A single attribution
 * page is allowed where there are several providers, so the footer keeps
 * only the short lines and links to it (UK legal audit 2026-09-27, LEG-19).
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
 * has the full record; the source PDFs no longer exist in this repo, see
 * LEG-17) found each agreement's own Schedule 1 Section 8 "ATTRIBUTION"
 * field independently either names a specific required wording or is blank
 * (general "give appropriate credit... in any reasonable manner" clause
 * only). Per feed:
 *   - Darwin Real Time Train Information (Push), the LDBWS/live-departure-
 *     boards source: Schedule 1 requires "powered by NationalRail" verbatim
 *     (lowercase "powered", one word "NationalRail"), linked to
 *     nationalrail.co.uk as a courtesy.
 *   - NationalRail Knowledgebase Stations (JSON): Schedule 1 requires
 *     "NationalRail (Train Information Services Ltd)" verbatim, as plain
 *     text. Production is provisioned under exactly this product
 *     (`1010-nationalrail-knowledgebase-stations-feed-_json_` in the
 *     production GitOps config, confirmed by the 2026-09-27 legal audit,
 *     LEG-23), not the blank-attribution "Stations Reference Data" product,
 *     so this wording applies.
 *   - Knowledgebase Incidents: Schedule 1 blank; Data Publisher is Rail
 *     Delivery Group, NOT National Rail Enquiries. Credited by name on the
 *     attribution page under the general "any reasonable manner" clause.
 *   - Knowledgebase TOC data: Schedule 1 blank. Same as Incidents.
 * The two required strings ARE concatenated onto one line: they share the
 * word "NationalRail", so "powered by NationalRail (Train Information
 * Services Ltd)" contains BOTH required strings intact and complete.
 * Neither string is altered. Only "powered by NationalRail" is linked.
 *
 * Network Rail Infrastructure Limited's own open-data feeds (TRUST, which
 * powers individual train tracking) are a THIRD, distinct licence. The
 * NRIL data feeds licence prescribes the statement "Contains Information of
 * Network Rail Infrastructure Limited licensed under the following
 * licence", hyperlinked to the licence (LEG-19). It is rendered verbatim
 * below with that link. Network Rail also forbids its brand or logo and any
 * claim to be "official", so the line stays unbranded.
 *
 * The timetable (CIF, from the DTD/RSP feed) needs "Source: RSP" or a link
 * to the Rail Delivery Group (LEG-22); both are given on the attribution
 * page.
 *
 * Operating basis (decision of 2026-09-27): the operator treats the current
 * and planned use of the Rail Data Marketplace feeds as permitted under the
 * RDM terms. The attribution here is therefore final, not a placeholder
 * awaiting Schedule 1 sign-off: the Darwin/KB Stations wording above, the
 * NRIL statement for TRUST (LEG-19), "Source: RSP" for CIF (LEG-22), and the
 * same National Rail line directly under LDBWS-derived departure data
 * (`NationalRailCredit`, LEG-23). Revisit only if the RDM terms change.
 *
 * Irish and Northern Irish sources (LEG-20, LEG-21): Irish Rail GTFS is the
 * NTA's, under CC BY 4.0, which requires the NTA's name, a link and an
 * "as is" statement. Translink station data comes from OpenDataNI under OGL
 * v3.0, whose default statement is used. The Irish Rail realtime API has no
 * published licence, so it gets a courtesy credit only.
 *
 * A plain Server Component with no interactivity, rendered once by the root
 * layout so it is on every page. */

export const OGL_V3_URL = 'https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/';
export const CC_BY_4_URL = 'https://creativecommons.org/licenses/by/4.0/';
export const NRIL_LICENCE_URL =
  'https://www.networkrail.co.uk/who-we-are/transparency-and-ethics/transparency/open-data-feeds/network-rail-infrastructure-limited-data-feeds-licence/';
export const TFL_TERMS_URL = 'https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service';

/** The exact prescribed NRIL statement. The whole phrase is the link text. */
export const NRIL_STATEMENT =
  'Contains Information of Network Rail Infrastructure Limited licensed under the following licence';

/** Non-affiliation statement (LEG-25). Also used by `/terms`. */
export const NON_AFFILIATION_STATEMENT =
  'Distant Signal is an independent, unofficial service and is not affiliated with or endorsed by National Rail, Network Rail, Rail Delivery Group, TfL, the National Transport Authority, Iarnród Éireann, Translink or any train operator.';

/** A credit or licence link. `tone="inherit"`: it sits in dimmed (footer)
 * or statement text whose wording is licence text, so it keeps that
 * colour and is marked by its underline (docs/style-guide.md "TextLink"). */
function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <TextLink href={href} target="_blank" rel="noopener noreferrer" underline="always" inline tone="inherit">
      {children}
    </TextLink>
  );
}

/** The legal links are decided at render time. Routes Next prerenders at
 * build time (`○` in the `next build` route list: today `/stations`,
 * `/lines/new`, `/groups/new`, `/journeys/new`, `/chat/callback`) render
 * this footer once, with the flags as the build saw them, so they only
 * show the legal links if the flag was set for the image build. Every
 * per-request route, including the legal pages and `/attribution`, reads
 * the runtime value. */
export function OpenDataAttribution() {
  const legalLinks = legalPagesVisible() ? LEGAL_LINKS : [];
  return (
    <Box component="footer" p="md" style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}>
      <Text size="xs" c="dimmed">
        Powered by TfL Open Data
      </Text>
      {/* `size="sm"`, not `xs`: review §2.16 ("auth controls are
          inconsistently sized") named this line specifically as "a 12px
          underlined link with a ~16px hit height", and `sm` (14px) is the
          size the chrome's other text-link-styled controls converge on
          (see `AuthStatus.tsx`). The other linked lines below follow it for
          the same reason; plain-text lines stay `xs`. */}
      <Text size="sm" c="dimmed">
        <ExternalLink href={NATIONAL_RAIL_URL}>powered by NationalRail</ExternalLink>
        {' (Train Information Services Ltd)'}
      </Text>
      <Text size="sm" c="dimmed">
        <ExternalLink href={NRIL_LICENCE_URL}>{NRIL_STATEMENT}</ExternalLink>
      </Text>
      <Text size="xs" c="dimmed">
        Also uses data from RSP, the National Transport Authority (Ireland), Iarnród Éireann and Translink via
        OpenDataNI. Independent and unofficial.
      </Text>
      <Group component="nav" aria-label="Site information" gap="md" mt={4}>
        <TextLink href="/attribution" size="sm" underline="always">
          Data sources and licences
        </TextLink>
        {legalLinks.map((link) => (
          <TextLink key={link.href} href={link.href} size="sm" underline="always">
            {link.label}
          </TextLink>
        ))}
      </Group>
    </Box>
  );
}

interface DataSource {
  /** Stable key, also used as the section's heading id. */
  id: string;
  title: string;
  /** What this app uses it for. */
  use: string;
  /** The required attribution, rendered verbatim. */
  statement: ReactNode;
  licence: ReactNode;
}

/** Every third-party source, with its required statement. Exported for
 * tests. Order: GB rail, London, Ireland, Northern Ireland, development. */
export const DATA_SOURCES: readonly DataSource[] = [
  {
    id: 'national-rail',
    title: 'National Rail (Darwin live departures and Knowledgebase)',
    use: 'Live departure boards, station information, incidents and train operator details, provided through the Rail Data Marketplace. Knowledgebase incidents and operator data are published by Rail Delivery Group.',
    statement: (
      <>
        <ExternalLink href={NATIONAL_RAIL_URL}>powered by NationalRail</ExternalLink>
        {' (Train Information Services Ltd)'}
      </>
    ),
    licence: 'Rail Data Marketplace data sharing agreements (adapted Open Government Licence v3.0).',
  },
  {
    id: 'network-rail',
    title: 'Network Rail (TRUST train movements)',
    use: 'Live train movements for individual train tracking.',
    statement: <ExternalLink href={NRIL_LICENCE_URL}>{NRIL_STATEMENT}</ExternalLink>,
    licence: (
      <>
        <ExternalLink href={NRIL_LICENCE_URL}>Network Rail Infrastructure Limited data feeds licence</ExternalLink>.
      </>
    ),
  },
  {
    id: 'network-rail-delay-attribution',
    title: 'Network Rail (delay attribution glossary)',
    use: 'The descriptions of the reason codes TRUST gives for a cancellation or a change of origin, from the Historic Delay Attribution Glossary (August 2021).',
    statement: (
      <>
        Contains information of Network Rail Infrastructure Limited licensed under the{' '}
        <ExternalLink href={OGL_V3_URL}>Open Government Licence v3.0</ExternalLink>.
      </>
    ),
    licence: <ExternalLink href={OGL_V3_URL}>Open Government Licence v3.0</ExternalLink>,
  },
  {
    id: 'rsp-timetable',
    title: 'Timetable (CIF)',
    use: 'Scheduled services and calling points from the rail industry timetable feed.',
    statement: (
      <>
        Source: RSP (<ExternalLink href="https://www.raildeliverygroup.com">Rail Delivery Group</ExternalLink>)
      </>
    ),
    licence: 'Rail Settlement Plan timetable data licence.',
  },
  {
    id: 'tfl',
    title: 'Transport for London',
    use: 'Transport for London line status.',
    statement: 'Powered by TfL Open Data',
    licence: (
      <>
        <ExternalLink href={TFL_TERMS_URL}>TfL transport data service terms</ExternalLink> (based on the Open Government
        Licence v2.0).
      </>
    ),
  },
  {
    id: 'nta-gtfs',
    title: 'Irish Rail timetable (GTFS)',
    use: 'Scheduled Irish Rail services.',
    statement: (
      <>
        Irish Rail timetable data ©{' '}
        <ExternalLink href="https://www.nationaltransport.ie">National Transport Authority</ExternalLink>, licensed
        under <ExternalLink href={CC_BY_4_URL}>CC BY 4.0</ExternalLink>. The GTFS data is provided &quot;as is&quot;,
        without warranty of any kind.
      </>
    ),
    licence: (
      <>
        <ExternalLink href={CC_BY_4_URL}>Creative Commons Attribution 4.0 International</ExternalLink>, under the{' '}
        <ExternalLink href="https://developer.nationaltransport.ie/usagepolicy">NTA usage policy</ExternalLink>.
      </>
    ),
  },
  {
    id: 'irish-rail-realtime',
    title: 'Iarnród Éireann realtime',
    use: 'Live Irish Rail running information.',
    statement: (
      <>
        Live Irish Rail data from <ExternalLink href="https://api.irishrail.ie/realtime/">Iarnród Éireann</ExternalLink>
        , provided &quot;as is&quot;.
      </>
    ),
    licence: 'No published licence; provided by Iarnród Éireann "as is".',
  },
  {
    id: 'opendatani',
    title: 'Translink (Northern Ireland Railways) via OpenDataNI',
    use: 'Northern Ireland stations and halts.',
    statement: (
      <>
        Contains public sector information licensed under the{' '}
        <ExternalLink href={OGL_V3_URL}>Open Government Licence v3.0</ExternalLink>. Translink data from{' '}
        <ExternalLink href="https://www.opendatani.gov.uk">OpenDataNI</ExternalLink>.
      </>
    ),
    licence: <ExternalLink href={OGL_V3_URL}>Open Government Licence v3.0</ExternalLink>,
  },
  {
    // Used only at development time: `reference-data/crs-tiploc.csv`, which
    // `crates/line-catalogue-validator` checks the line catalogue's station
    // codes against, is generated from the Rail Data Marketplace "NWR
    // CORPUS" extract. None of it is shown on the site. The statement is
    // OGL v3's default, as for the delay attribution glossary.
    id: 'network-rail-corpus',
    title: 'Network Rail (CORPUS location codes)',
    use: 'Used during development to check the station codes (CRS and TIPLOC) in our line catalogue. Not shown on this site.',
    statement: (
      <>
        Contains information of Network Rail Infrastructure Limited licensed under the{' '}
        <ExternalLink href={OGL_V3_URL}>Open Government Licence v3.0</ExternalLink>.
      </>
    ),
    licence: <ExternalLink href={OGL_V3_URL}>Open Government Licence v3.0</ExternalLink>,
  },
  {
    // No committed file comes from this site any more (station codes are
    // from CORPUS above, operator codes from the Knowledgebase TOC list),
    // but the line-catalogue validator's optional `--live` check still
    // reads its CRS pages (the scrape in
    // crates/line-catalogue-validator/src/reference.rs, `fetch_live`).
    // Nothing from it is shown on the site. See LEG-24 for the open
    // permission question. Remove this credit when that scrape is removed:
    // the validator's
    // `railwaycodes_credit_exists_exactly_while_the_live_tier_scrapes_it`
    // test fails while one exists without the other (it looks for
    // `id: 'railwaycodes'` here, so keep that id).
    id: 'railwaycodes',
    title: 'railwaycodes.org.uk',
    use: 'Occasionally used during development to cross-check station codes in our line catalogue. Not shown on this site.',
    statement: (
      <>
        Station codes cross-checked against{' '}
        <ExternalLink href="https://www.railwaycodes.org.uk">railwaycodes.org.uk</ExternalLink>.
      </>
    ),
    licence: 'All rights reserved by its owner.',
  },
];

/** Full content of `/attribution`. */
export function OpenDataAttributionDetails() {
  return (
    <Stack gap="lg">
      <Text>
        Distant Signal is built on open data. Each source below is credited with the wording its licence requires.
      </Text>
      {DATA_SOURCES.map((source) => (
        <Stack key={source.id} gap={4} component="section" aria-labelledby={`source-${source.id}`}>
          <SectionTitle id={`source-${source.id}`}>{source.title}</SectionTitle>
          <Text size="sm">{source.use}</Text>
          <Text size="sm" fw={500} data-attribution-statement>
            {source.statement}
          </Text>
          <Text size="sm" c="dimmed">
            Licence: {source.licence}
          </Text>
        </Stack>
      ))}
      <Stack gap={4} component="section" aria-labelledby="source-disclaimer">
        <SectionTitle id="source-disclaimer">Not an official service</SectionTitle>
        <Text size="sm">{NON_AFFILIATION_STATEMENT}</Text>
        <List size="sm">
          <ListItem>No data provider&apos;s logo or brand is used.</ListItem>
          <ListItem>
            Data is provided &quot;as is&quot; and may be late, incomplete or wrong. Check with the train operator
            before travelling.
          </ListItem>
        </List>
      </Stack>
    </Stack>
  );
}
