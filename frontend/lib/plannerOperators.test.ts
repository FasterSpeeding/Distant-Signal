import { describe, expect, it } from 'vitest';
import type { ComboboxItem, ComboboxParsedItem } from '@mantine/core';
import {
  joinNames,
  operatorChoices,
  operatorEntry,
  operatorFilterBanner,
  operatorFilterSummary,
  operatorList,
  operatorName,
  operatorSearchFilter,
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
