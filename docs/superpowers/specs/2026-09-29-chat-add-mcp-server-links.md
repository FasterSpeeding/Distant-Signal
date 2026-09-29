# /chat: add Distant Signal's MCP server to your own assistant

Date: 2026-09-29. Status: implemented (`frontend/components/AddMcpServerLinks.tsx`, `frontend/lib/mcpInstallLinks.ts`).

## Goal

Let a signed-in user add the Distant Signal MCP server to an assistant they already use, from `/chat`. The section shows
only when `railMcp.publicUrl` is configured (read at request time via `runtimeRailMcpPublicUrl()`).

## What the server needs from a client

Checked live against production on 2026-09-29:

- Endpoint: `{publicUrl}/mcp`, Streamable HTTP. It must be exactly that: the bare origin 404s, and the OAuth resource
  check is an exact match against `new URL('/mcp', publicUrl)`, so a trailing slash or a missing `/mcp` fails. Every
  user-facing copy (this section, `/connect-claude`, ChatPanel) builds it with `mcpEndpointUrl`. `/.well-known/oauth-protected-resource/mcp` gives
  `resource: https://ds-mcp.cursed.solutions/mcp`.
- OAuth 2.1 authorization server at `{publicUrl}/`: authorization code + PKCE (S256), public clients only
  (`token_endpoint_auth_methods_supported: ["none"]`), a `registration_endpoint` (DCR) and
  `client_id_metadata_document_supported: true` (CIMD).
- Sign-in goes to Authentik. Only members of the MCP users group get a token.

So any client that does MCP OAuth discovery plus DCR or CIMD should work with just the URL. None needs a client ID or
secret.

## Client research

"Verified" means the mechanism is in the client's current official docs (fetched 2026-09-29). It does not mean we
tested it end to end against Distant Signal. We haven't done that for any client.

| Client                                        | How to add a remote server                                                                                                                                     | One-click / deep link                                                                                                                   | OAuth fit                                                                                                                              | Verified                                                                                               |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Claude.ai (web, and mobile via the account)   | Customize > Connectors, then + > Add custom connector, paste URL, Add, Connect. Team/Enterprise: owner adds it under Organization settings > Connectors first. | No add-with-URL link. The docs link straight to `https://claude.ai/customize/connectors`.                                               | CIMD/DCR. Free plan: one custom connector.                                                                                             | Yes (support.claude.com 11175166)                                                                      |
| Claude Desktop                                | Same Connectors flow as Claude.ai (brokered by the account)                                                                                                    | None                                                                                                                                    | As above                                                                                                                               | Yes (same article)                                                                                     |
| Claude Code                                   | `claude mcp add --transport http <name> <url>`, then `/mcp` to sign in                                                                                         | None                                                                                                                                    | DCR and CIMD, both documented                                                                                                          | Yes (code.claude.com/docs/en/mcp)                                                                      |
| ChatGPT                                       | Settings > Security and login > Developer mode on, then Plugins > + > create a developer-mode app with the URL, auth OAuth                                     | None                                                                                                                                    | OAuth with CIMD and DCR documented. Plus, Pro, Business, Enterprise, Edu, on the web. Business/Enterprise admins must enable it first. | Yes (developers.openai.com developer-mode guide). The help-center article returned 403 to our fetcher. |
| Codex CLI                                     | `codex mcp add <name> --url <url>`, then `codex mcp login <name>`                                                                                              | None                                                                                                                                    | OAuth login documented                                                                                                                 | Yes (learn.chatgpt.com/docs/extend/mcp)                                                                |
| Cursor                                        | `mcp.json` `{ "mcpServers": { name: { "url": … } } }`                                                                                                          | Yes: `cursor://anysphere.cursor-deeplink/mcp/install?name=$NAME&config=$BASE64` (config = base64 of the server entry, e.g. `{"url":…}`) | Uses DCR by default. Docs offer static `auth` only for providers without DCR. Desktop redirect is `http://localhost:8787/callback`.    | Yes (cursor.com/docs/mcp/install-links). Forum reports of DCR bugs in some versions are unverified.    |
| VS Code (Copilot)                             | `mcp.json` `{ "type": "http", "url": … }`; `code --add-mcp '{json}'`                                                                                           | Yes: `vscode:mcp/install?{urlencoded JSON}` (Insiders: `vscode-insiders:`)                                                              | DCR first, then falls back to client credentials                                                                                       | Yes (code.visualstudio.com/api/extension-guides/ai/mcp)                                                |
| Windsurf (now under Devin Desktop, "Cascade") | `mcp_config.json` with `serverUrl`                                                                                                                             | None. The docs say Cascade has no one-click install.                                                                                    | OAuth "supported for each transport type" (no details)                                                                                 | Yes (docs.devin.ai/desktop/cascade/mcp)                                                                |
| LM Studio                                     | `mcp.json` `url` entry                                                                                                                                         | Yes: `lmstudio://add_mcp?name=…&config=<base64>`                                                                                        | Docs mention only header or bearer tokens, not OAuth                                                                                   | Deeplink yes. OAuth not documented, so not listed on the page.                                         |
| Gemini CLI                                    | `gemini mcp add --transport http <name> <url>`, then `/mcp auth <name>`                                                                                        | None                                                                                                                                    | Discovery plus DCR documented                                                                                                          | Yes (geminicli.com/docs/tools/mcp-server)                                                              |

## What the page shows

A `section` card headed "Use Distant Signal in your own assistant". It appears on the allowed branch below the chat
panel, and on the forbidden branch too, because the MCP users group is separate from the chat allowlist. It contains:

1. A plain note that only accounts given MCP access can connect.
2. The MCP server URL in a read-only labelled field with a copy button.
3. One-click "Add to Cursor" and "Add to VS Code" links (server name `distant-signal`).
4. Copyable commands for Claude Code, Codex CLI and Gemini CLI, each with its sign-in step.
5. Short steps for Claude.ai/Desktop (with a link to `claude.ai/customize/connectors`, and to `/connect-claude`) and
   for ChatGPT.
6. A collapsed troubleshooting note: an "unregistered redirect_uri" sign-in error means the client used a different
   loopback host (localhost vs 127.0.0.1) than it registered with; remove and re-add the server.

`/connect-claude` keeps the Claude.ai/Desktop instructions and links here for other assistants, rather than repeating
the list.

Copy feedback is a visible `role="status"` `aria-live="polite"` line under each field ("Copied." or a "Couldn't
copy" message). The page adds no dependencies and no logos. Brand names are plain text.

Left out: LM Studio (OAuth not documented) and Windsurf (no deep link, and config-file editing is too much for this
card). Either is a one-line addition.
