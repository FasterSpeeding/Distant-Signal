'use client';

import { useState } from 'react';
import { Alert, Button, Group, PasswordInput, Stack, Text } from '@mantine/core';
import { getAnthropicApiKey, setAnthropicApiKey, clearAnthropicApiKey } from '@/lib/anthropicKey';

/** The Chat page's own settings affordance for a user's Anthropic API key
 * (client-side-tokens design doc, Decision 6). Deliberately inline inside
 * /chat, not a new top-level route -- a single-field control, not a
 * page-sized concern.
 *
 * The disclosure text below is a real trust requirement, not polish: the
 * whole point of this redesign is that the key never reaches a Distant
 * Signal server, which only has value if the user is told it's true. */
export function AnthropicKeySettings() {
  const [hasKey, setHasKey] = useState(() => getAnthropicApiKey() !== null);
  const [input, setInput] = useState('');
  const [savedMessage, setSavedMessage] = useState(false);

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
        {hasKey ? 'Key saved.' : 'No key set.'}
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
        {hasKey && (
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
