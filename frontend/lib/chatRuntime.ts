import type Anthropic from '@anthropic-ai/sdk';
import type { runChatTurn } from './chatTurn';
import type { classifyChatError } from './chatErrors';

/** What the chat needs from the Anthropic and MCP SDKs at run time. */
export interface ChatRuntime {
  Anthropic: typeof Anthropic;
  runChatTurn: typeof runChatTurn;
  classifyChatError: typeof classifyChatError;
}

let pending: Promise<ChatRuntime> | null = null;

/** Loads the chat's SDK-backed half on demand. The Anthropic SDK and the MCP
 * client are about 140 KB of gzipped JS that /chat only needs once a
 * question is sent, so they live in their own chunk rather than the page's
 * first load. ChatPanel starts this when the question box gets focus, so the
 * chunk is usually there before Send. A failed load is forgotten, so the
 * next call tries again. The SDKs stay in the browser: the key and MCP
 * tokens they use are the visitor's own, held in this browser only. */
export function loadChatRuntime(): Promise<ChatRuntime> {
  pending ??= Promise.all([import('@anthropic-ai/sdk'), import('./chatTurn'), import('./chatErrors')]).then(
    ([sdk, turn, errors]) => ({
      Anthropic: sdk.default,
      runChatTurn: turn.runChatTurn,
      classifyChatError: errors.classifyChatError,
    }),
    (err: unknown) => {
      pending = null;
      throw err;
    },
  );
  return pending;
}
