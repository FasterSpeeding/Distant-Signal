type SeverityGroup = 'good' | 'informational' | 'planned' | 'mild' | 'severe';

const SEVERITY_TABLE: Record<number, { label: string; group: SeverityGroup }> = {
  0: { label: 'Special Service', group: 'informational' },
  1: { label: 'Closed', group: 'severe' },
  2: { label: 'Suspended', group: 'severe' },
  3: { label: 'Part Suspended', group: 'severe' },
  4: { label: 'Planned Closure', group: 'planned' },
  5: { label: 'Part Closure', group: 'planned' },
  6: { label: 'Severe Delays', group: 'severe' },
  7: { label: 'Reduced Service', group: 'mild' },
  8: { label: 'Rail Replacement', group: 'severe' },
  9: { label: 'Minor Delays', group: 'mild' },
  10: { label: 'Good Service', group: 'good' },
  11: { label: 'Part Closed', group: 'severe' },
  12: { label: 'Exit Only', group: 'informational' },
  13: { label: 'No Step Free Access', group: 'informational' },
  14: { label: 'Change of Frequency', group: 'mild' },
  20: { label: 'Recovering', group: 'mild' },
  21: { label: 'Diverted', group: 'severe' },
};

const GROUP_COLOR: Record<SeverityGroup, string> = {
  good: 'green',
  informational: 'gray',
  planned: 'blue',
  mild: 'yellow',
  severe: 'red',
};

export function severityColor(severity: number): string {
  const entry = SEVERITY_TABLE[severity];
  return entry ? GROUP_COLOR[entry.group] : 'gray';
}

export function severityLabel(severity: number): string {
  return SEVERITY_TABLE[severity]?.label ?? 'Unknown';
}
