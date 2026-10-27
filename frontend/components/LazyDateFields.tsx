'use client';

import { lazy, Suspense, type ComponentType, type ReactNode } from 'react';
import { TextInput } from '@mantine/core';
import type { DateInputProps, DatePickerInputProps, DatePickerType, DateTimePickerProps } from '@mantine/dates';

// `@mantine/dates`' calendar fields (and the calendar, popover and dayjs
// code behind them) are a sizeable chunk that only a few forms use, several
// of them inside modals. These wrappers load that chunk on demand
// (`dateFields.impl.ts`), so it stays out of every page's first load. Use
// them in place of the `@mantine/dates` imports; `TimeInput` is a plain
// input with no calendar, so it stays a direct import.
//
// Until the chunk arrives the field renders as a read-only `TextInput` with
// the same label, description and placeholder, so it takes the same space
// and nothing around it moves. On the server React waits for the chunk and
// streams the real field, so a field visible on load arrives as it did
// before; hydrating it waits for the chunk too.

const loadFields = () => import('./dateFields.impl');

// `lazy` needs a plain component type; the wrappers below keep the real
// (generic) prop types at the call sites.
type AnyProps = Record<string, unknown>;
const LazyDateInput = lazy(() => loadFields().then((m) => ({ default: m.DateInput as ComponentType<AnyProps> })));
const LazyDatePickerInput = lazy(() =>
  loadFields().then((m) => ({ default: m.DatePickerInput as ComponentType<AnyProps> })),
);
const LazyDateTimePicker = lazy(() =>
  loadFields().then((m) => ({ default: m.DateTimePicker as ComponentType<AnyProps> })),
);

interface FieldShape {
  label?: ReactNode;
  description?: ReactNode;
  placeholder?: string | undefined;
  required?: boolean | undefined;
  withAsterisk?: boolean | undefined;
  error?: ReactNode;
  disabled?: boolean | undefined;
}

/** Same box as the real field: a picker input is a Mantine `Input` too. */
function FieldPlaceholder({ label, description, placeholder, required, withAsterisk, error, disabled }: FieldShape) {
  return (
    <TextInput
      label={label}
      description={description}
      error={error}
      {...(placeholder !== undefined && { placeholder })}
      {...(required !== undefined && { required })}
      {...(withAsterisk !== undefined && { withAsterisk })}
      {...(disabled !== undefined && { disabled })}
      readOnly
      aria-busy
    />
  );
}

export function DateInput(props: DateInputProps) {
  return (
    <Suspense fallback={<FieldPlaceholder {...props} />}>
      <LazyDateInput {...(props as AnyProps)} />
    </Suspense>
  );
}

export function DatePickerInput<Type extends DatePickerType = 'default'>(props: DatePickerInputProps<Type>) {
  return (
    <Suspense fallback={<FieldPlaceholder {...props} />}>
      <LazyDatePickerInput {...(props as AnyProps)} />
    </Suspense>
  );
}

export function DateTimePicker(props: DateTimePickerProps) {
  return (
    <Suspense fallback={<FieldPlaceholder {...props} />}>
      <LazyDateTimePicker {...(props as AnyProps)} />
    </Suspense>
  );
}
