import { describe, expect, it } from 'vitest';
import { issueListEntries, withIssueListParams } from './issueListFilters';

describe('withIssueListParams', () => {
  it('adds the live issue filters, keeping the link params and hash', () => {
    const live = new URLSearchParams('dir=down&issueTab=all&issueSeverity=6&issueSeverity=9&tflIssueSource=tfl');
    expect(withIssueListParams('/lines/wcml?dir=up&at=10%3A00#trains', live)).toBe(
      '/lines/wcml?dir=up&at=10%3A00&issueTab=all&issueSeverity=6&issueSeverity=9&tflIssueSource=tfl#trains',
    );
  });

  it('replaces filters the link was built with by the live ones', () => {
    const live = new URLSearchParams('issueSource=planned');
    expect(withIssueListParams('/lines/wcml?issueTab=active&issueSource=tfl#trains', live)).toBe(
      '/lines/wcml?issueSource=planned#trains',
    );
  });

  it('leaves a link alone when no filters are set', () => {
    expect(withIssueListParams('/lines/wcml#trains', new URLSearchParams('view=routes&other=1'))).toBe(
      '/lines/wcml#trains',
    );
  });
});

describe('issueListEntries', () => {
  it('lists only the issue list params, in URL order', () => {
    const live = new URLSearchParams('at=10%3A00&tflIssueTab=ended&dir=up&issueSeverity=6&issueSeverity=9');
    expect(issueListEntries(live)).toEqual([
      ['tflIssueTab', 'ended'],
      ['issueSeverity', '6'],
      ['issueSeverity', '9'],
    ]);
  });
});
