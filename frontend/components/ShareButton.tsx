'use client';

import { ActionIcon, Tooltip } from '@mantine/core';
import { CheckIcon } from './CheckIcon';
import { ShareIcon } from './ShareIcon';
import { useShareOrCopy } from './useShareOrCopy';
import { tokens } from '@/lib/theme';

const DEFAULT_LABEL = 'Share this page';
// Task 3.6.10: the visible Tooltip text only -- reassures a visitor on a
// canonical-URL page (e.g. `/train/[uid]/[date]`, one of this button's
// four callers alongside stations/lines/incidents) that sharing from here
// really does hand out that page's own stable, permanent link, not some
// page-internal id. Deliberately NOT folded into `DEFAULT_LABEL` itself:
// that string is also this button's `aria-label` (its one accessible
// name), and every existing caller/test keys off that exact short string
// -- lengthening it would be a breaking rename for no accessibility
// benefit, since the extra context belongs in the tooltip's on-hover
// description, not the name. Kept generic (not train-specific) since the
// same button/wording is shared by all four page kinds.
const DEFAULT_TOOLTIP = 'Share this page (copies its permanent link)';
const COPIED_LABEL = 'Copied';

/** The page-level share button (stations, lines, incidents, trains):
 * `useShareOrCopy({ share: true })`, so the Web Share API where it exists
 * and the clipboard elsewhere, with a cancelled share doing nothing. Only
 * the clipboard path flips the label to "Copied" for a couple of seconds;
 * the OS share sheet gives its own feedback.
 *
 * Reads `window.location.href` at click time rather than taking a `url`
 * prop: this is a client component, and all four pages that render it
 * already resolve to clean, stable, shareable URLs with nothing to
 * reconstruct from query params. */
export function ShareButton() {
  const { state, run } = useShareOrCopy({ share: true });
  const copied = state === 'copied';
  const label = copied ? COPIED_LABEL : DEFAULT_LABEL;
  // The visible tooltip text and the accessible name deliberately diverge
  // while idle (see `DEFAULT_TOOLTIP`). Once copied, both read "Copied":
  // the extra context is no longer useful once the action has happened.
  const tooltipText = copied ? COPIED_LABEL : DEFAULT_TOOLTIP;

  return (
    <Tooltip label={tooltipText}>
      {/* `tokens.iconButton.pageAction` (44px), not Mantine's default
          `md` (28px): review §2.10 names this and `PinToggle`'s star as
          the two actions a returning visitor taps most. */}
      <ActionIcon
        variant="outline"
        color="gray"
        onClick={() => void run(window.location.href, document.title)}
        aria-label={label}
        size={tokens.iconButton.pageAction}
      >
        {copied ? <CheckIcon /> : <ShareIcon />}
      </ActionIcon>
    </Tooltip>
  );
}
