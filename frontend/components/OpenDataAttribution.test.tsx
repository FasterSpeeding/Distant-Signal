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

  it('is a landmark, so it is reachable rather than just visible', () => {
    const { container } = renderWithMantine(<OpenDataAttribution />);
    expect(container.querySelector('footer')).not.toBeNull();
  });
});
