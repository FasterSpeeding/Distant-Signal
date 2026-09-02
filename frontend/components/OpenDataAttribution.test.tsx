import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { OpenDataAttribution } from './OpenDataAttribution';

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
    // product) -- see the plan doc's Task 1, Step 2.
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('powered by NationalRail');
    expect(link.parentElement).toHaveTextContent('powered by NationalRail (Train Information Services Ltd)');
  });

  it('is a landmark, so it is reachable rather than just visible', () => {
    const { container } = renderWithMantine(<OpenDataAttribution />);
    expect(container.querySelector('footer')).not.toBeNull();
  });
});
