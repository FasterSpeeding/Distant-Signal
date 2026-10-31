'use client';

import { MultiSelect, type ComboboxProps } from '@mantine/core';
import { noOptionsFound } from './noOptionsFound';

/** The Trains section's Operator filter: any of the picked operators
 * (`operator=SW,GW`). Its options are `operatorChoices`' ("SW · South
 * Western Railway", then any unnamed code). Shared by `/trains`'
 * `TrainSearchForm` and a journey leg's window search (`LegFilterFields`),
 * so both read and behave the same. */
export function TrainOperatorSelect({
  data,
  value,
  onChange,
  comboboxProps,
}: {
  data: { value: string; label: string }[];
  value: string[];
  onChange: (value: string[]) => void;
  /** e.g. `{ withinPortal: false }` inside a popover. */
  comboboxProps?: ComboboxProps | undefined;
}) {
  return (
    <MultiSelect
      label="Operator (optional)"
      placeholder="Any operator"
      description="Only trains run by one of these operators."
      data={data}
      value={value}
      onChange={onChange}
      searchable
      clearable
      // Mantine hides the whole dropdown when it has no options and no
      // `nothingFoundMessage`, leaving the input `aria-expanded="true"`
      // over nothing.
      nothingFoundMessage={noOptionsFound('No matching operators')}
      clearButtonProps={{ 'aria-label': 'Clear operator filter', className: 'iconHitArea24' }}
      {...(comboboxProps && { comboboxProps })}
    />
  );
}
