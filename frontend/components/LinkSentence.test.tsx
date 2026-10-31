import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LinkSentence } from './LinkSentence';
import { LineStatusRow, LineStatusRowItem, LineStatusRowList } from './LineStatusRow';

describe('LinkSentence', () => {
  it('joins the links with commas and "and", notes in brackets, ending in a full stop', () => {
    const { container } = renderWithMantine(
      <LinkSentence
        lead="Change for trams at "
        links={[
          { href: '/a', label: 'Altrincham', note: 'Manchester Metrolink' },
          { href: '/b', label: 'Bury' },
          { href: '/c', label: 'Cornbrook' },
        ]}
      />,
    );
    const sentence = container.querySelector('[data-link-sentence]')!;
    expect(sentence.textContent).toBe('Change for trams at Altrincham (Manchester Metrolink), Bury and Cornbrook.');
    expect(screen.getByRole('link', { name: 'Bury' }).getAttribute('href')).toBe('/b');
  });

  it('says one link plainly, and nothing for none', () => {
    const { container, rerender } = renderWithMantine(
      <LinkSentence lead="Also here: " links={[{ href: '/v', label: 'Victoria' }]} />,
    );
    expect(container.querySelector('[data-link-sentence]')?.textContent).toBe('Also here: Victoria.');
    rerender(<LinkSentence lead="Also here: " links={[]} />);
    expect(container.querySelector('[data-link-sentence]')).toBeNull();
  });
});

describe('LineStatusRow', () => {
  it('is a list of cards, each named by its line and linking to it, with its badge and detail', () => {
    renderWithMantine(
      <LineStatusRowList label="Lines at this station">
        <LineStatusRowItem>
          <LineStatusRow href="/lines/x" name="East Coast Main Line" badge={<span>Minor delays</span>}>
            <span>On time: 92%</span>
          </LineStatusRow>
        </LineStatusRowItem>
      </LineStatusRowList>,
    );
    const list = screen.getByRole('list', { name: 'Lines at this station' });
    expect(list.querySelectorAll('li')).toHaveLength(1);
    expect(screen.getByRole('link', { name: 'East Coast Main Line' }).getAttribute('href')).toBe('/lines/x');
    expect(list.textContent).toContain('Minor delays');
    expect(list.textContent).toContain('On time: 92%');
    // No colour given, no swatch.
    expect(list.querySelector('[data-line-swatch]')).toBeNull();
  });
});
