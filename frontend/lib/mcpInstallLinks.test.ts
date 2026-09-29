import { describe, expect, it } from 'vitest';
import {
  claudeCodeCommand,
  codexCommand,
  cursorInstallLink,
  geminiCommand,
  mcpEndpointUrl,
  vscodeInstallLink,
} from './mcpInstallLinks';

const ENDPOINT = 'https://ds-mcp.example.com/mcp';

describe('mcpInstallLinks', () => {
  it('appends /mcp to the public URL with no trailing slash, tolerating trailing slashes on the input', () => {
    expect(mcpEndpointUrl('https://ds-mcp.example.com')).toBe(ENDPOINT);
    expect(mcpEndpointUrl('https://ds-mcp.example.com//')).toBe(ENDPOINT);
  });

  it('builds a Cursor deeplink whose config is the base64 mcp.json entry', () => {
    const link = cursorInstallLink(ENDPOINT);
    expect(link).toMatch(/^cursor:\/\/anysphere\.cursor-deeplink\/mcp\/install\?name=distant-signal&config=/);
    const config = new URL(link).searchParams.get('config') ?? '';
    expect(JSON.parse(atob(config))).toEqual({ url: ENDPOINT });
  });

  it('builds a VS Code install link from URL-encoded JSON', () => {
    const link = vscodeInstallLink(ENDPOINT);
    expect(link.startsWith('vscode:mcp/install?')).toBe(true);
    expect(JSON.parse(decodeURIComponent(link.slice('vscode:mcp/install?'.length)))).toEqual({
      name: 'distant-signal',
      type: 'http',
      url: ENDPOINT,
    });
  });

  it('builds the Claude Code, Codex CLI and Gemini CLI commands', () => {
    expect(claudeCodeCommand(ENDPOINT)).toBe(`claude mcp add --transport http distant-signal ${ENDPOINT}`);
    expect(codexCommand(ENDPOINT)).toBe(`codex mcp add distant-signal --url ${ENDPOINT}`);
    expect(geminiCommand(ENDPOINT)).toBe(`gemini mcp add --transport http distant-signal ${ENDPOINT}`);
  });
});
