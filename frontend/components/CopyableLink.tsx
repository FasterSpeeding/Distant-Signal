'use client';

import { useEffect, type ReactNode } from 'react';
import { ActionIcon, Group, Stack, Text, TextInput, Tooltip } from '@mantine/core';
import { CheckIcon } from './CheckIcon';
import { CopyIcon } from './CopyIcon';
import { ShareIcon } from './ShareIcon';
import { useShareOrCopy } from './useShareOrCopy';
import { tokens } from '@/lib/theme';

/** A read-only field holding a link or command, with one button that
 * copies it (or, with `share`, opens the device's share sheet first):
 * group invite links, journey share links, the MCP server URL and install
 * commands, the Claude connector URL.
 *
 * - The field is a real `TextInput`, so the value can be selected by hand
 *   if the clipboard refuses; focusing it selects the whole value.
 * - The button is `tokens.iconButton.field` (36px) beside the 36px input,
 *   outline gray, with a tooltip and an `aria-label` of "Copy <label>" or
 *   "Share <label>". Its icon turns to a tick while "Copied" shows.
 * - The result is announced in a polite live region under the field:
 *   "Copied." or "Couldn't copy. Select the text and copy it yourself."
 *
 * `label` names the field. Pass `hideLabel` where a heading or modal title
 * right above already says it: the label then becomes the input's
 * `aria-label` instead of visible text. */
export function CopyableLink({
  value,
  label,
  hideLabel = false,
  description,
  share = false,
  shareTitle,
  monospace = false,
  onCopied,
}: {
  value: string;
  label: string;
  hideLabel?: boolean | undefined;
  description?: ReactNode | undefined;
  /** Try the Web Share API before the clipboard (`useShareOrCopy`). */
  share?: boolean | undefined;
  shareTitle?: string | undefined;
  /** For a command rather than a URL. */
  monospace?: boolean | undefined;
  /** Called each time the value is copied or shared. */
  onCopied?: (() => void) | undefined;
}) {
  const { state, run } = useShareOrCopy({ share, ...(shareTitle !== undefined && { shareTitle }) });
  const copied = state === 'copied';
  useEffect(() => {
    if (copied) onCopied?.();
  }, [copied, onCopied]);
  // "Copy MCP server URL"; a hidden label reads mid-sentence ("Share invite link").
  const named = hideLabel ? label.charAt(0).toLowerCase() + label.slice(1) : label;
  const actionLabel = `${share ? 'Share' : 'Copy'} ${named}`;

  return (
    <Stack gap={4} data-copy-field>
      <Group gap="xs" wrap="nowrap" align="flex-end">
        <TextInput
          value={value}
          readOnly
          flex={1}
          miw={0}
          onFocus={(event) => event.currentTarget.select()}
          {...(hideLabel ? { 'aria-label': label } : { label })}
          {...(description !== undefined && { description })}
          {...(monospace && { ff: 'monospace' })}
        />
        <Tooltip label={copied ? 'Copied' : actionLabel} withArrow>
          <ActionIcon
            variant="outline"
            color="gray"
            size={tokens.iconButton.field}
            onClick={() => void run(value)}
            aria-label={actionLabel}
          >
            {copied ? <CheckIcon /> : share ? <ShareIcon /> : <CopyIcon />}
          </ActionIcon>
        </Tooltip>
      </Group>
      <Text role="status" aria-live="polite" size="xs" c={state === 'failed' ? 'var(--ds-color-error-text)' : 'dimmed'}>
        {copied && 'Copied.'}
        {state === 'failed' && "Couldn't copy. Select the text and copy it yourself."}
      </Text>
    </Stack>
  );
}
