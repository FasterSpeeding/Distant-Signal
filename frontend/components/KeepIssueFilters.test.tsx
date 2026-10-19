import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { IssueFiltersHiddenInputs, IssueFiltersLink, IssueFiltersTextLink } from './KeepIssueFilters';

const navigation = vi.hoisted(() => ({ search: null as string | null }));

vi.mock('next/navigation', () => ({
  useSearchParams: () => (navigation.search === null ? null : new URLSearchParams(navigation.search)),
}));

beforeEach(() => {
  navigation.search = null;
});

describe('KeepIssueFilters', () => {
  it('carries the live issue filters on a link', () => {
    navigation.search = 'issueTab=all&issueSeverity=6&dir=down';
    renderWithMantine(
      <IssueFiltersLink href="/lines/wcml?dir=up#trains" aria-current="page">
        Towards Euston
      </IssueFiltersLink>,
    );
    const link = screen.getByRole('link', { name: 'Towards Euston' });
    expect(link).toHaveAttribute('href', '/lines/wcml?dir=up&issueTab=all&issueSeverity=6#trains');
    expect(link).toHaveAttribute('aria-current', 'page');
  });

  it('carries them on a text link', () => {
    navigation.search = 'tflIssueSource=tfl';
    renderWithMantine(
      <IssueFiltersTextLink href="/lines/wcml?at=10%3A00#trains" ariaLabel="Later trains">
        Later
      </IssueFiltersTextLink>,
    );
    expect(screen.getByRole('link', { name: 'Later trains' })).toHaveAttribute(
      'href',
      '/lines/wcml?at=10%3A00&tflIssueSource=tfl#trains',
    );
  });

  it('keeps the href as built outside the app router', () => {
    renderWithMantine(<IssueFiltersLink href="/lines/wcml?issueTab=active#trains">By route</IssueFiltersLink>);
    expect(screen.getByRole('link', { name: 'By route' })).toHaveAttribute(
      'href',
      '/lines/wcml?issueTab=active#trains',
    );
  });

  it('adds a hidden input per filter value to a form', () => {
    navigation.search = 'from=EUS&issueSeverity=6&issueSeverity=9';
    const { container } = renderWithMantine(
      <form>
        <IssueFiltersHiddenInputs />
      </form>,
    );
    const inputs = [...container.querySelectorAll<HTMLInputElement>('input[type="hidden"]')];
    expect(inputs.map((input) => [input.name, input.value])).toEqual([
      ['issueSeverity', '6'],
      ['issueSeverity', '9'],
    ]);
  });
});
