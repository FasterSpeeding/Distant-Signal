import { describe, expect, it } from 'vitest';
import type { ComboboxItem, ComboboxParsedItem } from '@mantine/core';
import {
  joinNames,
  journeyOperatorFilterLine,
  savedOperatorFilter,
  operatorChoices,
  operatorEntry,
  operatorFilterBanner,
  operatorFilterSummary,
  operatorList,
  operatorName,
  operatorOnlyLabel,
  operatorSearchFilter,
  preferOutcomeNote,
} from './plannerOperators';
import type { PlannerOperatorCatalogue } from './types';

const catalogue: PlannerOperatorCatalogue = {
  operators: [
    { code: 'GR', name: 'LNER' },
    { code: 'LF', name: null },
    { code: 'LM', name: 'LNR & WMR' },
    { code: 'LT', name: 'London Underground' },
    { code: 'VT', name: 'Avanti West Coast' },
  ],
  groups: [
    {
      code: 'GTR',
      name: 'GTR (Thameslink, Great Northern, Southern, Gatwick Express)',
      members: ['TL', 'GN', 'SN', 'GX'],
    },
  ],
  dsOperators: [
    { code: 'ds:DLR', name: 'DLR' },
    { code: 'ds:LONDON_TRAMS', name: 'London Trams' },
  ],
};

describe('operator entries', () => {
  it('normalizes ATOC codes, ds: operators and groups, and drops the rest', () => {
    expect(operatorEntry(' vt ')).toBe('VT');
    expect(operatorEntry('DS:dlr')).toBe('ds:DLR');
    expect(operatorEntry('Group:gtr')).toBe('group:GTR');
    for (const bad of ['', 'V', 'VTX', 'ds:', 'group:G', 'x:VT']) expect(operatorEntry(bad)).toBeNull();
  });

  it('reads a URL list, deduplicated and capped at 8', () => {
    expect(operatorList('vt,VT,,bad!,group:gtr')).toEqual(['VT', 'group:GTR']);
    expect(operatorList('A1,A2,A3,A4,A5,A6,A7,A8,A9')).toHaveLength(8);
    expect(operatorList(undefined)).toEqual([]);
  });
});

describe('operatorChoices', () => {
  it('lists groups, then operators by name (unnamed by code, last), then DS operators', () => {
    const choices = operatorChoices(catalogue);
    expect(choices.map((section) => section.group)).toEqual(['Groups', 'Operators', 'Trams and light rail']);
    expect(choices[0]!.items[0]).toMatchObject({ value: 'group:GTR', label: expect.stringMatching(/^GTR \(/) });
    expect(choices[1]!.items.map((item) => item.label)).toEqual([
      'VT · Avanti West Coast',
      'GR · LNER',
      'LM · LNR & WMR (West Midlands Railway and London Northwestern Railway)',
      'LT · London Underground (the Tube, and its trains on National Rail lines)',
      'LF',
    ]);
    expect(choices[2]!.items.map((item) => item.value)).toEqual(['ds:DLR', 'ds:LONDON_TRAMS']);
  });

  it('finds an operator by an alias it is never picked by', () => {
    const choices = operatorChoices(catalogue);
    const filter = operatorSearchFilter(choices);
    const options: ComboboxParsedItem[] = choices.map((section) => ({
      group: section.group,
      items: section.items.map(({ value, label }) => ({ value, label })),
    }));
    const found = (search: string) =>
      filter({ options, search, limit: 100 }).flatMap((option) =>
        'group' in option ? option.items.map((item: ComboboxItem) => item.value) : [option.value],
      );
    expect(found('west midlands')).toEqual(['LM']);
    expect(found('tube')).toEqual(['LT']);
    expect(found('thameslink')).toEqual(['group:GTR']);
    expect(found('dlr')).toEqual(['ds:DLR']);
    expect(found('nothing like this')).toEqual([]);
  });
});

describe('operator names', () => {
  const nameOf = (entry: string) => operatorName(entry, catalogue);

  it('names codes, groups, DS operators and the Underground', () => {
    expect(nameOf('VT')).toBe('Avanti West Coast');
    expect(nameOf('group:GTR')).toBe('GTR');
    expect(nameOf('ds:DLR')).toBe('DLR');
    expect(nameOf('LT')).toBe('London Underground');
    expect(nameOf('LF')).toBe('LF');
    expect(operatorName('VT', null)).toBe('VT');
  });

  it('joins names in plain English', () => {
    expect(joinNames([])).toBe('');
    expect(joinNames(['A'])).toBe('A');
    expect(joinNames(['A', 'B'])).toBe('A and B');
    expect(joinNames(['A', 'B', 'C'])).toBe('A, B and C');
  });

  it('summarizes a filter for the collapsed Advanced options', () => {
    expect(operatorFilterSummary('only', ['VT', 'GR'], nameOf)).toBe('Only Avanti West Coast, LNER');
    expect(operatorFilterSummary('avoid', ['group:GTR'], nameOf)).toBe('Avoid GTR');
    expect(operatorFilterSummary(undefined, ['VT'], nameOf)).toBeNull();
    expect(operatorFilterSummary('only', [], nameOf)).toBeNull();
  });

  it('says what an applied filter means for the results', () => {
    expect(operatorFilterBanner({ operator: ['VT'], avoidOperator: [] }, nameOf)).toBe(
      'Showing routes using only Avanti West Coast. Walks between stations are included; the Tube is not.',
    );
    expect(operatorFilterBanner({ operator: ['VT', 'LT'] }, nameOf)).toBe(
      'Showing routes using only Avanti West Coast and London Underground.',
    );
    expect(operatorFilterBanner({ avoidOperator: ['LT'] }, nameOf)).toMatch(
      /^Showing routes that avoid London Underground, including the Tube/,
    );
    // The request's own entries name a group as a group.
    expect(operatorFilterBanner({ avoidOperator: ['TL', 'GN', 'SN', 'GX'] }, nameOf, ['group:GTR'])).toBe(
      'Showing routes that avoid GTR.',
    );
    expect(operatorFilterBanner({ operator: [], avoidOperator: [] }, nameOf)).toBeNull();
  });
});

describe('Prefer (2026-10-31)', () => {
  const nameOf = (entry: string) => operatorName(entry, catalogue);

  it('labels the operator-only option, a group by its name', () => {
    expect(operatorOnlyLabel(['GR'], nameOf)).toBe('LNER only');
    expect(operatorOnlyLabel(['VT', 'GR'], nameOf)).toBe('Avanti West Coast or LNER only');
    expect(operatorOnlyLabel(['TL', 'GN', 'SN', 'GX'], nameOf, ['group:GTR'])).toBe('GTR only');
  });

  it('summarizes Prefer and explains an option it could not add', () => {
    expect(operatorFilterSummary('prefer', ['GR'], nameOf)).toBe('Prefer LNER');
    expect(preferOutcomeNote('tooSlow', 'LNER')).toBe(
      "The best route using only LNER is much slower than these, so it isn't shown.",
    );
    expect(preferOutcomeNote('none', 'LNER')).toBe('No route uses only LNER from this time.');
    expect(preferOutcomeNote('added', 'LNER')).toBeNull();
    expect(preferOutcomeNote(undefined, 'LNER')).toBeNull();
  });
});

describe('savedOperatorFilter', () => {
  it('keeps what the plan applied, with the search entries when the mode matches', () => {
    expect(
      savedOperatorFilter({ operator: ['TL', 'GN', 'SN', 'GX'] }, { operatorMode: 'only', operators: ['group:GTR'] }),
    ).toEqual({
      mode: 'only',
      entries: ['group:GTR'],
    });
    expect(savedOperatorFilter({ avoidOperator: ['LT'] }, { operatorMode: 'only', operators: ['VT'] })).toEqual({
      mode: 'avoid',
      entries: ['LT'],
    });
    expect(savedOperatorFilter({ operator: [], avoidOperator: [], preferOperator: ['GR'] }, null)).toEqual({
      mode: 'prefer',
      entries: ['GR'],
    });
    expect(
      savedOperatorFilter({ operator: [], avoidOperator: [] }, { operatorMode: 'only', operators: ['VT'] }),
    ).toBeNull();
  });
});

describe('journeyOperatorFilterLine', () => {
  const nameOf = (entry: string) => operatorName(entry, catalogue);
  it('words each mode', () => {
    expect(journeyOperatorFilterLine({ mode: 'only', entries: ['VT', 'GR'] }, nameOf)).toBe(
      'Planned using only Avanti West Coast and LNER',
    );
    expect(journeyOperatorFilterLine({ mode: 'avoid', entries: ['group:GTR'] }, nameOf)).toBe('Planned avoiding GTR');
    expect(journeyOperatorFilterLine({ mode: 'prefer', entries: ['GR', 'VT'] }, nameOf)).toBe(
      'Planned preferring LNER or Avanti West Coast',
    );
  });
});
