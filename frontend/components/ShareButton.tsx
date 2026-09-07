'use client';

import { useState } from 'react';
import { ActionIcon, Tooltip } from '@mantine/core';

/** The classic three-node "share" glyph (Feather's `share-2`), matching
 * `StarIcon` (PinToggle.tsx) and `InfoIcon.tsx`'s stroke conventions:
 * 24x24 viewBox, `currentColor` stroke, `aria-hidden` (the accessible name
 * lives on the `ActionIcon` wrapping it, which differs per state below). */
function ShareIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="18" cy="5" r="3" />
      <circle cx="6" cy="12" r="3" />
      <circle cx="18" cy="19" r="3" />
      <line x1="8.59" y1="13.51" x2="15.42" y2="17.49" />
      <line x1="15.41" y1="6.51" x2="8.59" y2="10.49" />
    </svg>
  );
}

const DEFAULT_LABEL = 'Share this page';
const COPIED_LABEL = 'Copied!';
// Long enough to read, short enough that the button doesn't feel stuck.
const COPIED_TIMEOUT_MS = 2000;

/** Web Share API where it exists (most mobile browsers), falling back to
 * copying the URL to the clipboard everywhere else -- notably desktop
 * Firefox, which has never shipped `navigator.share`, and any other
 * browser lacking it. Feature-detected per click rather than once at
 * module load, since it's a cheap check and this stays correct even if a
 * test or polyfill swaps it out between renders.
 *
 * A user cancelling the native share sheet rejects the `share()` promise
 * with an `AbortError` -- that's the person changing their mind, not a
 * failure, so it neither falls back to the clipboard nor shows any error
 * state. Any *other* rejection (no share target configured, permission
 * denied, etc.) does fall back to the clipboard, on the theory that some
 * copy of the link beats a silent dead click.
 *
 * No transient state for the `navigator.share` path -- the OS's own share
 * sheet already gives feedback that the action happened. Only the
 * clipboard fallback flips the label to "Copied!" for a couple of
 * seconds, mirroring how PinToggle.tsx flips its label on state change.
 *
 * Reads `window.location.href` at click time rather than taking a `url`
 * prop: this is a client component, and all four pages that render it
 * already resolve to clean, stable, shareable URLs with nothing to
 * reconstruct from query params. */
export function ShareButton() {
  const [copied, setCopied] = useState(false);

  async function share() {
    const url = window.location.href;
    if (typeof navigator.share === 'function') {
      try {
        await navigator.share({ url, title: document.title });
        return;
      } catch (err) {
        // A cancelled share rejects with a `DOMException` named
        // `AbortError` -- but per the WebIDL spec `DOMException` does NOT
        // extend `Error` (confirmed here: jsdom's implementation doesn't,
        // and not every browser's does either), so `instanceof Error`
        // would silently miss it and fall through to the clipboard for a
        // plain cancel. Checking `.name` directly works regardless of the
        // rejection's actual prototype chain.
        if (err && typeof err === 'object' && 'name' in err && err.name === 'AbortError') {
          return;
        }
        // Fall through to the clipboard fallback below.
      }
    }
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), COPIED_TIMEOUT_MS);
    } catch {
      // Clipboard write failed too (e.g. permission denied) -- nothing
      // more to do; the button just doesn't show the "Copied!" state.
    }
  }

  const label = copied ? COPIED_LABEL : DEFAULT_LABEL;

  return (
    <Tooltip label={label}>
      <ActionIcon variant="outline" color="gray" onClick={share} aria-label={label}>
        <ShareIcon />
      </ActionIcon>
    </Tooltip>
  );
}
