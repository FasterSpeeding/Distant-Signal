import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import * as legal from '@/lib/legal';
import {
  CC_BY_4_URL,
  DATA_SOURCES,
  NON_AFFILIATION_STATEMENT,
  NRIL_LICENCE_URL,
  NRIL_STATEMENT,
  OGL_V3_URL,
  OpenDataAttribution,
  OpenDataAttributionDetails,
} from './OpenDataAttribution';

vi.mock('@/lib/legal', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/legal')>();
  return { ...actual, legalPagesVisible: vi.fn(actual.legalPagesVisible) };
});

afterEach(() => {
  vi.mocked(legal.legalPagesVisible).mockReset();
  delete process.env.LEGAL_PAGES_PUBLISHED;
  delete process.env.LEGAL_PAGES_PREVIEW;
});

describe('OpenDataAttribution', () => {
  it('carries TfL\'s required attribution verbatim', () => {
    // Not decoration: TfL's modified OGL v2.0 requires this exact phrase
    // wherever its open data is presented. Reworded, it stops being
    // attribution.
    renderWithMantine(<OpenDataAttribution />);
    expect(screen.getByText('Powered by TfL Open Data')).toBeInTheDocument();
  });

  it("carries the Darwin (LDBWS) feed's required attribution verbatim, linked to nationalrail.co.uk", () => {
    // Not decoration: the Darwin Real Time Train Information (Push)
    // Data Sharing Agreement's Schedule 1 §8 fixes this exact string --
    // lowercase "powered", one word "NationalRail" -- see
    // docs/superpowers/plans/2026-09-01-rdm-attribution-wording.md.
    // This wording is specific to the Darwin/LDBWS feed, not an umbrella
    // NRE claim covering every RDM feed this app consumes. Only this
    // phrase is linked -- the Knowledgebase Stations text appended right
    // after it (see below) carries no link requirement of its own.
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('powered by NationalRail');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', 'https://www.nationalrail.co.uk');
  });

  it("carries the Knowledgebase Stations feed's required attribution verbatim, concatenated onto the Darwin line", () => {
    // NationalRail Knowledgebase Stations (JSON)'s Schedule 1 §8 fixes
    // this exact string. Revised 2026-09-02: appended directly after the
    // Darwin line's "powered by NationalRail" rather than its own
    // separate line -- the two required strings share the word
    // "NationalRail", so concatenating them keeps BOTH verbatim strings
    // intact and complete within the rendered text (see
    // OpenDataAttribution.tsx's own comment for the exact substring
    // argument). Asserting on the wrapping element's full text content,
    // not a `getByText` exact match, since the required phrase now spans
    // the linked node plus a trailing plain-text node. Conditional: the
    // audit did not confirm this is the actual product this app's
    // Stations subscription is provisioned under (vs. the
    // differently-scoped, blank-attribution "Stations Reference Data"
    // product) -- see the plan doc's Task 1, Step 2. Confirmed by the
    // 2026-09-27 legal audit (LEG-23): production uses the KB Stations JSON
    // product, so this wording applies.
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('powered by NationalRail');
    expect(link.parentElement).toHaveTextContent('powered by NationalRail (Train Information Services Ltd)');
  });

  it('is a landmark, so it is reachable rather than just visible', () => {
    const { container } = renderWithMantine(<OpenDataAttribution />);
    expect(container.querySelector('footer')).not.toBeNull();
  });

  // Review §2.16 "auth controls are inconsistently sized" named this line
  // specifically: "a 12px underlined link with a ~16px hit height". Bumped
  // to `sm` (14px), the size the chrome's other text-link-styled controls
  // (AuthStatus's "Log in") converge on -- its two plain-text sibling lines
  // stay at `xs`, since they carry no link of their own.
  it("renders the NationalRail attribution line (the one with a link) at sm, not the xs plain-text lines use", () => {
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('powered by NationalRail');
    expect(link.parentElement).toHaveStyle({ '--text-fz': 'var(--mantine-font-size-sm)' });
    expect(screen.getByText('Powered by TfL Open Data')).not.toHaveStyle({
      '--text-fz': 'var(--mantine-font-size-sm)',
    });
  });

  it("carries Network Rail's prescribed NRIL statement verbatim, linked to the licence (LEG-19)", () => {
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByRole('link', { name: NRIL_STATEMENT });
    expect(link).toHaveAttribute('href', NRIL_LICENCE_URL);
    expect(NRIL_STATEMENT).toBe(
      'Contains Information of Network Rail Infrastructure Limited licensed under the following licence',
    );
  });

  it('links to the /attribution page from a labelled footer nav', () => {
    renderWithMantine(<OpenDataAttribution />);
    const nav = screen.getByRole('navigation', { name: 'Site information' });
    expect(within(nav).getByRole('link', { name: 'Data sources and licences' })).toHaveAttribute('href', '/attribution');
  });

  it('hides the legal page links while the legal pages are unpublished (the default)', () => {
    renderWithMantine(<OpenDataAttribution />);
    for (const name of ['Privacy', 'Terms', 'Cookies', 'Contact', 'Accessibility']) {
      expect(screen.queryByRole('link', { name })).toBeNull();
    }
  });

  it('keeps the legal links hidden when the flag is on but placeholders remain', () => {
    process.env.LEGAL_PAGES_PUBLISHED = 'true';
    renderWithMantine(<OpenDataAttribution />);
    expect(screen.queryByRole('link', { name: 'Privacy' })).toBeNull();
  });

  it('shows the legal page links in preview mode, so reviewers can reach the drafts', () => {
    process.env.LEGAL_PAGES_PREVIEW = 'true';
    renderWithMantine(<OpenDataAttribution />);
    expect(screen.getByRole('link', { name: 'Privacy' })).toHaveAttribute('href', '/privacy');
  });

  it('shows the legal page links once the legal pages are published', () => {
    vi.mocked(legal.legalPagesVisible).mockReturnValue(true);
    renderWithMantine(<OpenDataAttribution />);
    for (const [name, href] of [
      ['Privacy', '/privacy'],
      ['Terms', '/terms'],
      ['Cookies', '/cookies'],
      ['Contact', '/contact'],
      ['Accessibility', '/accessibility'],
    ]) {
      expect(screen.getByRole('link', { name })).toHaveAttribute('href', href);
    }
  });
});

describe('OpenDataAttributionDetails (/attribution)', () => {
  const statementOf = (id: string) => {
    const heading = document.getElementById(`source-${id}`);
    expect(heading).not.toBeNull();
    const section = heading!.closest('section')!;
    return section.querySelector('[data-attribution-statement]') as HTMLElement;
  };

  it('gives every data source a heading and a statement', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    for (const source of DATA_SOURCES) {
      expect(screen.getByRole('heading', { level: 2, name: source.title })).toBeInTheDocument();
      expect(statementOf(source.id).textContent).not.toBe('');
    }
  });

  it('carries the TfL and National Rail wording verbatim', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    expect(statementOf('tfl')).toHaveTextContent(/^Powered by TfL Open Data$/);
    expect(statementOf('national-rail')).toHaveTextContent(/^powered by NationalRail \(Train Information Services Ltd\)$/);
  });

  it('carries the prescribed NRIL statement, linked to the licence', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    const link = within(statementOf('network-rail')).getByRole('link', { name: NRIL_STATEMENT });
    expect(link).toHaveAttribute('href', NRIL_LICENCE_URL);
  });

  it('credits the CIF timetable as "Source: RSP" with a Rail Delivery Group link', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    const statement = statementOf('rsp-timetable');
    expect(statement).toHaveTextContent(/^Source: RSP/);
    expect(within(statement).getByRole('link', { name: 'Rail Delivery Group' })).toHaveAttribute(
      'href',
      'https://www.raildeliverygroup.com',
    );
  });

  it("meets CC BY 4.0 for the NTA's Irish Rail GTFS: provider name, licence link and 'as is'", () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    const statement = statementOf('nta-gtfs');
    expect(within(statement).getByRole('link', { name: 'National Transport Authority' })).toBeInTheDocument();
    expect(within(statement).getByRole('link', { name: 'CC BY 4.0' })).toHaveAttribute('href', CC_BY_4_URL);
    expect(statement).toHaveTextContent('provided "as is"');
  });

  it('uses the OGL v3 statement for Translink data from OpenDataNI', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    const statement = statementOf('opendatani');
    expect(statement).toHaveTextContent(
      'Contains public sector information licensed under the Open Government Licence v3.0.',
    );
    expect(within(statement).getByRole('link', { name: 'Open Government Licence v3.0' })).toHaveAttribute(
      'href',
      OGL_V3_URL,
    );
    expect(statement).toHaveTextContent('Translink data from OpenDataNI');
  });

  it('credits Iarnród Éireann and railwaycodes.org.uk', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    expect(statementOf('irish-rail-realtime')).toHaveTextContent('Iarnród Éireann');
    expect(statementOf('railwaycodes')).toHaveTextContent('railwaycodes.org.uk');
  });

  it('states that the service is unofficial and unaffiliated', () => {
    renderWithMantine(<OpenDataAttributionDetails />);
    expect(screen.getByText(NON_AFFILIATION_STATEMENT)).toBeInTheDocument();
  });

  it('opens every external link in a new tab without an opener', () => {
    const { container } = renderWithMantine(<OpenDataAttributionDetails />);
    for (const a of container.querySelectorAll('a[href^="http"]')) {
      expect(a).toHaveAttribute('target', '_blank');
      expect(a).toHaveAttribute('rel', 'noopener noreferrer');
    }
  });
});
