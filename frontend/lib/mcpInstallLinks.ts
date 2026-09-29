/** Install links and commands for adding Distant Signal's MCP server to a
 * user's own assistant. Every format here is the one each client documents
 * (see docs/superpowers/specs/2026-09-29-chat-add-mcp-server-links.md):
 *
 * - Cursor: `cursor://anysphere.cursor-deeplink/mcp/install?name=…&config=…`,
 *   `config` being the base64 of the server's `mcp.json` entry.
 * - VS Code: `vscode:mcp/install?…`, the URL-encoded JSON of
 *   `{name, type, url}`.
 * - Claude Code: `claude mcp add --transport http <name> <url>`.
 * - Codex CLI: `codex mcp add <name> --url <url>`.
 * - Gemini CLI: `gemini mcp add --transport http <name> <url>`, then
 *   `/mcp auth <name>` inside Gemini CLI to sign in (Gemini CLI's
 *   docs/tools/mcp-server.md).
 *
 * All of them take the Streamable HTTP endpoint, `{publicUrl}/mcp` -- the
 * `resource` the server's own protected-resource metadata advertises, and
 * the same URL ChatPanel connects to. It has to be exactly that: the bare
 * origin has no route (404), and the server's OAuth resource check is an
 * exact string match, so a trailing slash or a missing `/mcp` fails
 * sign-in. Every user-facing copy of the URL (/chat, /connect-claude,
 * ChatPanel) goes through `mcpEndpointUrl` so they can't drift apart. */

/** The name each client stores the server under. Lower-case and
 * hyphenated so it also works as a CLI argument without quoting. */
export const MCP_SERVER_NAME = 'distant-signal';

/** `railMcp.publicUrl` → the MCP endpoint, `{publicUrl}/mcp`, with no
 * trailing slash. Tolerates trailing slashes on `publicUrl`. */
export function mcpEndpointUrl(publicUrl: string): string {
  return `${publicUrl.replace(/\/+$/, '')}/mcp`;
}

export function cursorInstallLink(endpoint: string): string {
  const config = btoa(JSON.stringify({ url: endpoint }));
  return `cursor://anysphere.cursor-deeplink/mcp/install?name=${encodeURIComponent(MCP_SERVER_NAME)}&config=${encodeURIComponent(config)}`;
}

export function vscodeInstallLink(endpoint: string): string {
  const config = { name: MCP_SERVER_NAME, type: 'http', url: endpoint };
  return `vscode:mcp/install?${encodeURIComponent(JSON.stringify(config))}`;
}

export function claudeCodeCommand(endpoint: string): string {
  return `claude mcp add --transport http ${MCP_SERVER_NAME} ${endpoint}`;
}

export function codexCommand(endpoint: string): string {
  return `codex mcp add ${MCP_SERVER_NAME} --url ${endpoint}`;
}

export function geminiCommand(endpoint: string): string {
  return `gemini mcp add --transport http ${MCP_SERVER_NAME} ${endpoint}`;
}
