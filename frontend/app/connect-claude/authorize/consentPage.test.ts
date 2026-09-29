import { afterEach, describe, expect, it } from 'vitest';
import { within } from '@testing-library/react';
import { renderConsentScreen, renderErrorPage } from './consentPage';

/** Loads a rendered response into jsdom's own document, so the page can be
 * queried by role and accessible name exactly as a screen reader sees it. */
async function load(res: Response): Promise<Document> {
  document.documentElement.innerHTML = '';
  const parsed = new DOMParser().parseFromString(await res.text(), 'text/html');
  document.replaceChild(document.importNode(parsed.documentElement, true), document.documentElement);
  return document;
}

afterEach(() => {
  document.documentElement.innerHTML = '<head></head><body></body>';
});

describe('renderConsentScreen', () => {
  it('renders one h1 naming the client, inside a <main> landmark', async () => {
    const doc = await load(renderConsentScreen({ mcpRequestId: 'req1', clientName: 'Claude', nonce: null }));
    const main = within(doc.body).getByRole('main');
    const headings = within(doc.body).getAllByRole('heading');
    expect(headings).toHaveLength(1);
    expect(within(main).getByRole('heading', { level: 1, name: 'Connect Claude to Distant Signal' })).toBeTruthy();
    expect(doc.title).toBe('Connect Claude — Distant Signal');
    expect(doc.documentElement.getAttribute('lang')).toBe('en');
  });

  it('keeps the form posting back to the same URL with approve/deny decision buttons', async () => {
    const doc = await load(renderConsentScreen({ mcpRequestId: 'req-1_A', nonce: null }));
    const form = doc.querySelector('form')!;
    expect(form.getAttribute('method')).toBe('POST');
    expect(form.getAttribute('action')).toBe('/connect-claude/authorize?mcp_request_id=req-1_A');
    const approve = within(form).getByRole('button', { name: 'Approve' });
    const deny = within(form).getByRole('button', { name: 'Deny' });
    expect(approve.getAttribute('type')).toBe('submit');
    expect(approve.getAttribute('name')).toBe('decision');
    expect(approve.getAttribute('value')).toBe('approve');
    expect(deny.getAttribute('name')).toBe('decision');
    expect(deny.getAttribute('value')).toBe('deny');
  });

  it('offers a skip link to the main content and a brand link home', async () => {
    const doc = await load(renderConsentScreen({ mcpRequestId: 'req1', nonce: null }));
    expect(within(doc.body).getByRole('link', { name: 'Skip to content' }).getAttribute('href')).toBe('#main-content');
    expect(doc.getElementById('main-content')?.tagName).toBe('MAIN');
    expect(within(doc.body).getByRole('link', { name: 'Distant Signal' }).getAttribute('href')).toBe('/');
  });

  it('falls back to a generic label when the client registered no name', async () => {
    const doc = await load(renderConsentScreen({ mcpRequestId: 'req1', nonce: null }));
    expect(within(doc.body).getByRole('heading', { name: 'Connect An application to Distant Signal' })).toBeTruthy();
  });

  it('escapes an untrusted client name everywhere it appears, title included', async () => {
    const res = renderConsentScreen({ mcpRequestId: 'req1', clientName: '<img src=x onerror=alert(1)>', nonce: null });
    const html = await res.text();
    expect(html).not.toContain('<img');
    expect(html.match(/&lt;img/g)?.length).toBe(3);
  });

  it('carries the per-request CSP nonce on its one inline script', async () => {
    const html = await renderConsentScreen({ mcpRequestId: 'req1', nonce: 'abc123+/=' }).text();
    const scripts = html.match(/<script[^>]*>/g) ?? [];
    expect(scripts).toEqual(['<script nonce="abc123+/=">']);
    expect(html).toContain('mantine-color-scheme-value');
  });

  it('renders no script at all without a nonce, or with a malformed one', async () => {
    for (const nonce of [null, '"><script>alert(1)</script>']) {
      const html = await renderConsentScreen({ mcpRequestId: 'req1', nonce }).text();
      expect(html).not.toContain('<script');
    }
  });

  it('styles both colour schemes from the theme tokens (grape primary, dark body)', async () => {
    const html = await renderConsentScreen({ mcpRequestId: 'req1', nonce: null }).text();
    // grape 7 filled/link in light (app/globals.css's contrast override),
    // Mantine's dark 7 body in dark, and an explicit stored choice wins.
    expect(html).toContain('--ds-filled: #ae3ec9');
    expect(html).toContain('--ds-body: #242424');
    expect(html).toContain("html[data-mantine-color-scheme='dark']");
    expect(html).toContain("html:not([data-mantine-color-scheme='light'])");
  });
});

describe('renderErrorPage', () => {
  it('keeps the status, and renders a heading plus a role="alert" message with a non-colour icon', async () => {
    const res = renderErrorPage({ status: 410, heading: 'Expired', message: 'Try again from Claude.', nonce: null });
    expect(res.status).toBe(410);
    expect(res.headers.get('content-type')).toBe('text/html; charset=utf-8');
    const doc = await load(res);
    expect(within(doc.body).getByRole('heading', { level: 1, name: 'Expired' })).toBeTruthy();
    const alert = within(doc.body).getByRole('alert');
    expect(alert.textContent).toContain('Try again from Claude.');
    expect(alert.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  });

  it('offers a way back into the app', async () => {
    const doc = await load(renderErrorPage({ status: 502, heading: 'Oops', message: 'x', nonce: null }));
    expect(within(doc.body).getByRole('link', { name: 'How to connect Claude' }).getAttribute('href')).toBe(
      '/connect-claude',
    );
    expect(within(doc.body).getByRole('link', { name: 'Back to the home page' }).getAttribute('href')).toBe('/');
  });
});
