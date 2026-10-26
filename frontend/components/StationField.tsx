'use client';

import { searchStations } from '@/lib/suggestions';
import { stationLabels, typedStationCode } from '@/lib/codeLabels';
import { suggestionDropdownLabel } from '@/lib/suggestionAutocomplete';
import { CodeLookupField, type CodeLookupFieldProps } from './CodeLookupField';

export type StationFieldProps = Omit<
  CodeLookupFieldProps,
  'labels' | 'optionLabel' | 'search' | 'noMatchMessage' | 'parseTypedCode'
> & {
  /** `searchStations` unless the field also offers stops or groups. */
  search?: CodeLookupFieldProps['search'] | undefined;
  /** Defaults to "No matching stations". */
  noMatchMessage?: string | undefined;
  /** Whether a typed code is picked on blur or Enter: `true` (the
   * default) for a 3-letter station code, or a parser of its own (the
   * planner's `tiploc:`, `group:` and TfL stop codes); `false` for only
   * listed suggestions. */
  allowTypedCode?: boolean | ((text: string) => string | null) | undefined;
};

/** A station (or stop, or group) field that shows "London Euston (EUS)"
 * once picked and hands its parent the code (`CodeLookupField`). */
export function StationField({
  search = searchStations,
  noMatchMessage = 'No matching stations',
  allowTypedCode = true,
  ...props
}: StationFieldProps) {
  const parseTypedCode =
    typeof allowTypedCode === 'function' ? allowTypedCode : allowTypedCode ? typedStationCode : undefined;
  return (
    <CodeLookupField
      placeholder="Station name or code"
      {...props}
      search={search}
      labels={stationLabels}
      optionLabel={suggestionDropdownLabel}
      noMatchMessage={noMatchMessage}
      parseTypedCode={parseTypedCode}
    />
  );
}
