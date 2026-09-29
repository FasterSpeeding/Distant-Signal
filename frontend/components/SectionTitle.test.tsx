import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { SectionTitle } from './SectionTitle';

/** Mantine's `Title` puts the chosen size on the element as a
 * `--title-fz` custom property naming the heading-size token. */
function sizeToken(el: HTMLElement): string {
  return el.style.getPropertyValue('--title-fz');
}

describe('SectionTitle', () => {
  it('renders an h2 at h3 size by default', () => {
    renderWithMantine(<SectionTitle id="download-heading">Download my data</SectionTitle>);
    const heading = screen.getByRole('heading', { level: 2, name: 'Download my data' });
    expect(heading).toHaveAttribute('id', 'download-heading');
    expect(sizeToken(heading)).toContain('--mantine-h3-font-size');
  });

  it('renders a subsection one level down at one size down', () => {
    renderWithMantine(<SectionTitle order={3}>Step-free access</SectionTitle>);
    const heading = screen.getByRole('heading', { level: 3, name: 'Step-free access' });
    expect(sizeToken(heading)).toContain('--mantine-h4-font-size');
  });

  it('renders order 4 at h5 size', () => {
    renderWithMantine(<SectionTitle order={4}>Lifts</SectionTitle>);
    expect(sizeToken(screen.getByRole('heading', { level: 4 }))).toContain('--mantine-h5-font-size');
  });
});
