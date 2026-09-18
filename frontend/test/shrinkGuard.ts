import { expect } from 'vitest';

/**
 * Regression guard for the WCAG 2.5.3 bug Task 1.5 fixed: a `Group
 * wrap="nowrap"` row pairing a title with a badge/button must give the
 * non-title element an explicit shrink guard (`flexShrink: 0`, or
 * `flex: "none"`) so a long title can't squeeze it down to nothing or off
 * -screen instead of the title itself truncating.
 *
 * `data-wrap="nowrap"` is this codebase's own DOM marker for such a row
 * (Mantine itself only ever turns the `wrap` prop into a `--group-wrap`
 * CSS var, never a plain attribute) — see `LineStatusCard.tsx` and
 * `components/StatusRow.tsx`, which both set it. Any row that wants this
 * assertion to see it has to carry the same marker.
 */
const NOWRAP_GROUP_SELECTOR = '[data-wrap="nowrap"]';

function hasShrinkGuard(node: HTMLElement): boolean {
  return node.style.flexShrink === '0' || node.style.flex === 'none';
}

/**
 * Asserts `element` (the badge/button itself, or any node inside it —
 * whatever `screen.getByText`/`getByRole` handed back) sits inside a
 * `data-wrap="nowrap"` row AND that somewhere between it and that row's
 * own element there is a shrink-guarded ancestor (or `element` itself is
 * one). Fails loudly, with the element's own markup, when either isn't
 * true — asserting the *effect* (the element can't be crushed), not the
 * mechanism, so it passes for any row that achieves the guard another way
 * than `StatusRow`.
 */
export function expectShrinkGuarded(element: HTMLElement): void {
  const group = element.closest(NOWRAP_GROUP_SELECTOR);
  if (!(group instanceof HTMLElement)) {
    expect.fail(
      `expected element to be inside a wrap="nowrap" row (an ancestor carrying data-wrap="nowrap"), but none was found. Element: ${element.outerHTML.slice(0, 200)}`,
    );
    return;
  }

  let node: HTMLElement | null = element;
  while (node && node !== group) {
    if (hasShrinkGuard(node)) return;
    node = node.parentElement;
  }

  expect.fail(
    `element has no shrink-guarded ancestor (flexShrink: 0 / flex: none) before reaching its wrap="nowrap" row — a long sibling title could squeeze it to nothing or off-screen (WCAG 2.5.3). Element: ${element.outerHTML.slice(0, 200)}`,
  );
}

/**
 * Sweeps every `data-wrap="nowrap"` row inside `container` and asserts
 * every `Badge` (`.mantine-Badge-root`) found inside one has a
 * shrink-guarded ancestor within that same row. This is the literal
 * "any Group wrap='nowrap' containing a Badge gives that badge a shrink
 * guard" check the task brief asked for, run against real rendered pages
 * rather than a synthetic fixture.
 *
 * Deliberately scoped to `Badge`s only (not every non-title child, e.g. a
 * bare action `Button`) — a container-wide sweep can't tell a row's
 * "title" from its "trailing" content in general, but a `Badge` is never
 * a row's title in this codebase, so any `Badge` found inside a
 * `wrap="nowrap"` row is unambiguously the element this bug crushes.
 * Buttons are checked individually via `expectShrinkGuarded` at the
 * specific call sites that render one (see `StatusRow.test.tsx` and the
 * page-level tests that assert on `RemoveGroupTrainButton` /
 * `RemoveCustomLineGrantButton` / `RenameTrainButton`).
 */
export function expectNoUnguardedNowrapBadges(container: HTMLElement): void {
  const groups = container.querySelectorAll(NOWRAP_GROUP_SELECTOR);
  for (const group of groups) {
    if (!(group instanceof HTMLElement)) continue;
    const badges = group.querySelectorAll('.mantine-Badge-root');
    for (const badge of badges) {
      if (badge instanceof HTMLElement) {
        expectShrinkGuarded(badge);
      }
    }
  }
}
