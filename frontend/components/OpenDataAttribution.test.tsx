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

  it("carries National Rail Enquiries' required attribution verbatim, linked to their site", () => {
    // Same posture as the TfL line above: NRE Developer Guidelines v06.01
    // §4 fixes this exact wording for all four RDM feeds this app consumes
    // (Incidents, LDBWS/Darwin, Stations, TOCs).
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('Powered by National Rail Enquiries');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', 'https://www.nationalrail.co.uk');
  });

  it('is a landmark, so it is reachable rather than just visible', () => {
    const { container } = renderWithMantine(<OpenDataAttribution />);
    expect(container.querySelector('footer')).not.toBeNull();
  });
});
