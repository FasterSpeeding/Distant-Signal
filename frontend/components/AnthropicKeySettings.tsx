'use client';

import { useEffect, useState } from 'react';
import { useMounted } from '@mantine/hooks';
import { Alert, Button, Group, PasswordInput, Stack, Text } from '@mantine/core';
import { getAnthropicApiKey, setAnthropicApiKey, clearAnthropicApiKey } from '@/lib/anthropicKey';

/** The Chat page's own settings affordance for a user's Anthropic API key
 * (client-side-tokens design doc, Decision 6). Deliberately inline inside
 * /chat, not a new top-level route -- a single-field control, not a
 * page-sized concern.
 *
 * The disclosure text below is a real trust requirement, not polish: the
 * whole point of this redesign is that the key never reaches a Distant
 * Signal server, which only has value if the user is told it's true.
 *
 * Same useMounted()-gated shape PrideToggle.tsx already uses for its own
 * localStorage-seeded state: `localStorage` doesn't exist during SSR (or
 * the client's first pre-hydration render), so `hasKey` starts at the
 * deterministic `false` every render agrees on, and only picks up the
 * real stored value from an effect gated on `mounted` -- never read
 * directly during render. */
export function AnthropicKeySettings() {
  const mounted = useMounted();
  const [hasKey, setHasKey] = useState(false);
  const [input, setInput] = useState('');
  const [savedMessage, setSavedMessage] = useState(false);

  useEffect(() => {
    if (!mounted) return;
    setHasKey(getAnthropicApiKey() !== null);
  }, [mounted]);

  const displayedHasKey = mounted ? hasKey : false;

  function handleSave() {
    if (!input.trim()) return;
    setAnthropicApiKey(input.trim());
    setInput('');
    setHasKey(true);
    setSavedMessage(true);
  }

  function handleClear() {
    clearAnthropicApiKey();
    setHasKey(false);
    setSavedMessage(false);
  }

  return (
    <Stack gap="xs">
      <Alert color="blue" variant="light">
        Your Anthropic API key is stored only in your browser (localStorage)
        and sent only to Anthropic directly when you chat -- it is never
        seen by any Distant Signal server.
      </Alert>
      <Text size="sm" fw={500}>
        {displayedHasKey ? 'Key saved.' : 'No key set.'}
      </Text>
      <Group align="flex-end">
        <PasswordInput
          label="Anthropic API key"
          placeholder="sk-ant-..."
          value={input}
          onChange={(e) => setInput(e.currentTarget.value)}
          style={{ flex: 1 }}
        />
        <Button onClick={handleSave} disabled={!input.trim()}>
          Save
        </Button>
        {displayedHasKey && (
          <Button variant="subtle" color="red" onClick={handleClear}>
            Clear
          </Button>
        )}
      </Group>
      {savedMessage && (
        <Text size="xs" c="dimmed">
          Saved to this browser.
        </Text>
      )}
    </Stack>
  );
}
