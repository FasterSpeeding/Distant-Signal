'use client';

import { searchTocs } from '@/lib/suggestions';
import { operatorLabels, typedOperatorCode } from '@/lib/codeLabels';
import { suggestionDropdownLabel } from '@/lib/suggestionAutocomplete';
import { CodeLookupField, type CodeLookupFieldProps } from './CodeLookupField';

export type OperatorFieldProps = Omit<
  CodeLookupFieldProps,
  'labels' | 'optionLabel' | 'search' | 'noMatchMessage' | 'parseTypedCode'
>;

/** An operator field that shows "Avanti West Coast (VT)" once picked and
 * hands its parent the 2-character code (`CodeLookupField`). */
export function OperatorField(props: OperatorFieldProps) {
  return (
    <CodeLookupField
      {...props}
      search={searchTocs}
      labels={operatorLabels}
      optionLabel={suggestionDropdownLabel}
      noMatchMessage="No matching operators"
      parseTypedCode={typedOperatorCode}
    />
  );
}
