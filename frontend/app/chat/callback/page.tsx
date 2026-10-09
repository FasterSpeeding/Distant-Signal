import { runtimeRailMcpPublicUrl } from '@/lib/csp';
import { ChatCallback } from './ChatCallback';
import type { Metadata } from 'next';

// Read the environment per request, never at build time.
export const dynamic = 'force-dynamic';

export const metadata: Metadata = { title: 'Connecting to the rail data service', robots: { index: false } };

/** `/chat/callback` -- a Server Component wrapper (FE-2) so the MCP
 * server's public URL is read from the runtime environment and handed to
 * the client-side OAuth code exchange as a prop. See `ChatCallback`. */
export default function ChatCallbackPage() {
  return <ChatCallback serverUrl={runtimeRailMcpPublicUrl()} />;
}
