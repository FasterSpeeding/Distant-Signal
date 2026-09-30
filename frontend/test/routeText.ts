import type { MatcherFunction } from '@testing-library/react';

/** An element's text as it shows on screen: `textContent` minus every
 * visually hidden span -- `RouteArrow`'s "to" (`components/RouteArrow.tsx`)
 * and `ExternalLinkIcon`'s "(opens in a new tab)", as a Mantine
 * `VisuallyHidden` or the sanitizers' `[data-visually-hidden]` -- so a
 * rendered route reads "KGX → YRK" and a credit reads its licence wording
 * exactly as they appear. Whitespace is collapsed the way Testing
 * Library's default normalizer does. */
export function visibleText(element: Element): string {
  const clone = element.cloneNode(true) as Element;
  clone.querySelectorAll(VISUALLY_HIDDEN).forEach((node) => node.remove());
  // The new-tab icon's own leading space is spacing for the icon, not part
  // of the wording: "Licence v3.0 [icon]." reads as "Licence v3.0.".
  clone.querySelectorAll('svg[data-icon="external-link"]').forEach((icon) => {
    const before = icon.previousSibling;
    if (before?.nodeType === 3 && before.textContent === ' ') before.remove();
  });
  return (clone.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** A `*ByText` matcher on `visibleText`, for route text: the arrow and the
 * hidden "to" are separate elements, so the default matcher (an element's
 * OWN text nodes) no longer sees "KGX → YRK" as one string. Matches the
 * innermost element whose visible text equals `expected` (a string) or
 * matches it (a RegExp), never its ancestors as well. */
export function byVisibleText(expected: string | RegExp): MatcherFunction {
  const matches = (element: Element) => {
    const text = visibleText(element);
    return typeof expected === 'string' ? text === expected : new RegExp(expected.source, expected.flags).test(text);
  };
  return (_content, element) =>
    element !== null && matches(element) && !Array.from(element.children).some((child) => matches(child));
}

const VISUALLY_HIDDEN = '[data-route-spoken], .mantine-VisuallyHidden-root, [data-visually-hidden]';
