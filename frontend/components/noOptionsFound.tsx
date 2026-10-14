import { Box } from '@mantine/core';

/** A `nothingFoundMessage` node, not a plain string: Mantine's own
 * `Combobox.Empty` (what `nothingFoundMessage` renders into,
 * `OptionsDropdown.tsx` in `@mantine/core`) is an unstyled `<Box>` with no
 * ARIA role at all, so a plain string leaves the `role="listbox"` wrapper
 * with a child that is neither an `option` nor a `group` -- axe's
 * `aria-required-children` (critical) fires on exactly that. Without any
 * `nothingFoundMessage`, Mantine instead hides the whole listbox while the
 * input keeps `aria-expanded="true"`. Both are reachable whenever a
 * `searchable` field's search matches nothing, not just with an empty
 * catalogue. `Combobox.Empty` forwards unrecognised props straight onto
 * that `<Box>`, so `role="option"` + `aria-disabled` here is enough to make
 * the listbox structurally valid without touching Mantine internals --
 * `aria-disabled` (state, not `disabled`) keeps it out of the tab order's
 * expectations without hiding it, matching how a real disabled option
 * would read. */
export function noOptionsFound(label: string) {
  return (
    <Box role="option" aria-disabled="true">
      {label}
    </Box>
  );
}
