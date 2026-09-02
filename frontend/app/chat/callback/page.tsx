'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Stack, Text, Title } from '@mantine/core';
import { auth } from '@modelcontextprotocol/sdk/client/auth.js';
import { BrowserMcpOAuthProvider } from '@/lib/mcpOAuthProvider';

/** `railMcp`'s own public URL, baked in at container-start -- the same
 * env var `frontend/app/connect-claude/page.tsx`'s own `railMcpPublicUrl()`
 * already reads for exactly this purpose. Read fresh (not module-level)
 * for the same "picked up per render, not baked at module-load" reasoning
 * that page's own doc comment gives. */
function railMcpPublicUrl(): string {
  const url = process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL;
  if (!url) throw new Error('NEXT_PUBLIC_RAILMCP_PUBLIC_URL is not configured on this deployment');
  return url;
}

/** `/chat/callback` -- the redirect target `distant-signal-mcp`'s own
 * `/authorize` -> `/connect-claude/authorize` consent bridge sends the
 * browser back to once the user approves (client-side-tokens design doc,
 * Decisions 1/3, Architecture step 3). Exchanges the `code` query param
 * for a bearer token via the MCP SDK's own `auth()` orchestrator
 * (`@modelcontextprotocol/sdk/client/auth.js`) -- the SAME function
 * `StreamableHTTPClientTransport` calls internally on a 401, reused here
 * directly for the one-time authorization-code exchange, driven by
 * `BrowserMcpOAuthProvider` (Task 7) so the resulting tokens land in
 * `localStorage` the same way either caller would leave them. */
export default function ChatCallbackPage() {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const code = new URLSearchParams(window.location.search).get('code');
    if (!code) {
      setError('No authorization code was present in the callback URL.');
      return;
    }

    const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
    auth(provider, { serverUrl: railMcpPublicUrl(), authorizationCode: code })
      .then((result) => {
        if (result === 'AUTHORIZED') {
          router.replace('/chat');
        } else {
          setError('Authorization did not complete. Please try connecting again from the Chat page.');
        }
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err.message : 'Connecting to the rail data service failed.');
      });
  }, [router]);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Connecting…</Title>
      {error ? <Text c="red">{error}</Text> : <Text c="dimmed">Finishing sign-in to the rail data service.</Text>}
    </Stack>
  );
}
