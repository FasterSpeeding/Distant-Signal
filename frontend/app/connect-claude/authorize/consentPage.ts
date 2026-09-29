import { NextResponse } from 'next/server';
import { DEFAULT_THEME } from '@mantine/core';

/** Server-rendered HTML for `/connect-claude/authorize` (route.ts): the
 * consent screen and the route's user-facing error pages.
 *
 * A Route Handler can't render a React tree, and this one has to stay a
 * Route Handler (it answers with 400/410/502 statuses a page can't set, and
 * the consent form POSTs back to the same URL). So, like `public/offline.html`,
 * this is a standalone document styled by hand. It still follows the app's
 * style guide rather than the browser defaults it used to fall back to:
 *
 * - every colour, size, radius and spacing value is read from Mantine's own
 *   `DEFAULT_THEME` tokens, not typed in as hex, with the app's contrast
 *   overrides from `app/globals.css` applied the same way (filled grape and
 *   links at grape 7 in the light scheme, dimmed text at gray 7 / dark 1);
 * - the page shape is the app's: the "Distant Signal" brand bar, a skip
 *   link, a `<main>` landmark, one `h1`, a bordered card like `/account`'s
 *   sections, a filled grape primary button next to a default one, and the
 *   same visible focus ring Mantine draws;
 * - light and dark follow the app's own colour-scheme handling: the stored
 *   Mantine choice (`mantine-color-scheme-value`, the key `ColorSchemeScript`
 *   reads) when a nonce is available to run the script, otherwise
 *   `prefers-color-scheme`, which is also Mantine's `auto` default.
 *
 * CSP: the page policy (lib/csp.ts) allows inline styles but only nonce'd
 * scripts. The one script here carries the per-request nonce proxy.ts
 * mints, and is left out entirely when there is none. */

const { colors, white, black, fontFamily, fontSizes, headings, radius, spacing, lineHeights } = DEFAULT_THEME;

/** The same shape lib/csp.ts accepts when building the policy. Anything else
 * is dropped rather than interpolated into an attribute. */
const NONCE_SHAPE = /^[A-Za-z0-9+/_-]+=*$/;

/** Escapes a value interpolated into HTML text or a double-quoted attribute. */
export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function schemeVars(scheme: 'light' | 'dark'): string {
  const v =
    scheme === 'light'
      ? {
          body: white,
          text: black,
          dimmed: colors.gray[7],
          surface: white,
          border: colors.gray[3],
          filled: colors.grape[7],
          filledHover: colors.grape[8],
          anchor: colors.grape[7],
          defaultBg: white,
          defaultHover: colors.gray[0],
          defaultBorder: colors.gray[4],
          defaultText: black,
          errorBg: colors.red[0],
          errorBorder: colors.red[2],
          errorText: colors.red[9],
        }
      : {
          body: colors.dark[7],
          text: colors.dark[0],
          dimmed: colors.dark[1],
          surface: colors.dark[6],
          border: colors.dark[4],
          filled: colors.grape[8],
          filledHover: colors.grape[9],
          anchor: colors.grape[4],
          defaultBg: colors.dark[6],
          defaultHover: colors.dark[5],
          defaultBorder: colors.dark[4],
          defaultText: white,
          errorBg: colors.dark[6],
          errorBorder: colors.red[4],
          errorText: colors.red[4],
        };
  return `color-scheme: ${scheme};
  --ds-body: ${v.body}; --ds-text: ${v.text}; --ds-dimmed: ${v.dimmed};
  --ds-surface: ${v.surface}; --ds-border: ${v.border};
  --ds-filled: ${v.filled}; --ds-filled-hover: ${v.filledHover}; --ds-anchor: ${v.anchor};
  --ds-default-bg: ${v.defaultBg}; --ds-default-hover: ${v.defaultHover};
  --ds-default-border: ${v.defaultBorder}; --ds-default-text: ${v.defaultText};
  --ds-error-bg: ${v.errorBg}; --ds-on-filled: ${white}; --ds-error-border: ${v.errorBorder}; --ds-error-text: ${v.errorText};`;
}

const STYLES = `
:root { --mantine-scale: 1; ${schemeVars('light')} }
@media (prefers-color-scheme: dark) {
  html:not([data-mantine-color-scheme='light']) { ${schemeVars('dark')} }
}
html[data-mantine-color-scheme='dark'] { ${schemeVars('dark')} }
*, *::before, *::after { box-sizing: border-box; }
body {
  margin: 0; min-height: 100vh; display: flex; flex-direction: column;
  font-family: ${fontFamily}; font-size: ${fontSizes.md}; line-height: ${lineHeights.md};
  background: var(--ds-body); color: var(--ds-text);
  -webkit-font-smoothing: antialiased;
}
a { color: var(--ds-anchor); text-decoration: none; }
a:hover, a:focus-visible { text-decoration: underline; }
:focus-visible { outline: 2px solid var(--ds-filled); outline-offset: 2px; }
.skip-link {
  position: absolute; left: -9999px; top: -9999px; z-index: 1;
  padding: ${spacing.sm}; border-radius: ${radius.sm};
  background: var(--ds-filled); color: var(--ds-on-filled);
}
.skip-link:focus-visible { left: ${spacing.md}; top: ${spacing.md}; outline-color: var(--ds-anchor); }
.site-header { border-bottom: 1px solid var(--ds-border); }
.site-header-inner { max-width: 1140px; margin: 0 auto; padding: ${spacing.sm} ${spacing.lg}; }
.brand { color: inherit; font-weight: 700; }
main { flex: 1; width: 100%; max-width: 640px; margin: 0 auto; padding: ${spacing.lg}; }
.card {
  display: flex; flex-direction: column; gap: ${spacing.md};
  padding: ${spacing.lg}; border: 1px solid var(--ds-border); border-radius: ${radius.md};
  background: var(--ds-surface);
}
h1 {
  margin: 0; font-family: ${headings.fontFamily}; font-weight: ${headings.fontWeight};
  font-size: ${headings.sizes.h2.fontSize}; line-height: ${headings.sizes.h2.lineHeight};
  overflow-wrap: anywhere;
}
p { margin: 0; overflow-wrap: anywhere; }
.dimmed { color: var(--ds-dimmed); font-size: ${fontSizes.sm}; }
form { margin: 0; }
.actions { display: flex; flex-wrap: wrap; gap: ${spacing.sm}; }
.button {
  font: inherit; font-size: ${fontSizes.sm}; font-weight: 600; line-height: 1;
  min-height: 36px; padding: 0 ${spacing.lg}; border-radius: ${radius.md};
  border: 1px solid transparent; cursor: pointer;
  display: inline-flex; align-items: center; justify-content: center;
}
.button:hover, .button:focus-visible { text-decoration: none; }
.button-filled { background: var(--ds-filled); color: var(--ds-on-filled); }
.button-filled:hover { background: var(--ds-filled-hover); }
.button-default { background: var(--ds-default-bg); color: var(--ds-default-text); border-color: var(--ds-default-border); }
.button-default:hover { background: var(--ds-default-hover); }
@media (max-width: 30em) {
  .actions .button { flex: 1 1 100%; }
}
.alert {
  display: flex; gap: ${spacing.sm}; padding: ${spacing.md};
  border: 1px solid var(--ds-error-border); border-radius: ${radius.md};
  background: var(--ds-error-bg);
}
.alert-icon { flex: none; color: var(--ds-error-text); margin-top: 2px; }
.links { display: flex; flex-wrap: wrap; gap: ${spacing.xs} ${spacing.lg}; }
`;

/** Mirrors Mantine's `ColorSchemeScript`: applies a stored explicit
 * light/dark choice before first paint. `auto` (or nothing stored) is left
 * to the `prefers-color-scheme` rule above. */
const COLOR_SCHEME_SCRIPT = `try{var s=localStorage.getItem('mantine-color-scheme-value');if(s==='light'||s==='dark'){document.documentElement.setAttribute('data-mantine-color-scheme',s);}}catch(e){}`;

/** The same exclamation-in-a-circle glyph `app/chat/callback/ChatCallback.tsx`
 * uses, so an error is not signalled by colour alone (WCAG 1.4.1). */
const ERROR_ICON = `<svg class="alert-icon" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>`;

function renderDocument({ title, nonce, body }: { title: string; nonce: string | null; body: string }): string {
  const script =
    nonce && NONCE_SHAPE.test(nonce) ? `\n<script nonce="${escapeHtml(nonce)}">${COLOR_SCHEME_SCRIPT}</script>` : '';
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<meta name="color-scheme" content="light dark">
<title>${title} — Distant Signal</title>${script}
<style>${STYLES}</style>
</head>
<body>
<a href="#main-content" class="skip-link">Skip to content</a>
<header class="site-header"><div class="site-header-inner"><a href="/" class="brand">Distant Signal</a></div></header>
<main id="main-content">
${body}
</main>
</body>
</html>`;
}

function htmlResponse(html: string, status: number): NextResponse {
  return new NextResponse(html, {
    status,
    headers: { 'Content-Type': 'text/html; charset=utf-8' },
  });
}

/** The consent screen. `clientName` is the DCR-registered client_name --
 * entirely self-reported by the connecting MCP client, never verified -- so
 * it is escaped here, the only untrusted value this page interpolates. */
export function renderConsentScreen({
  mcpRequestId,
  clientName,
  nonce,
}: {
  mcpRequestId: string;
  clientName?: string;
  nonce: string | null;
}): NextResponse {
  const name = clientName ? escapeHtml(clientName) : 'An application';
  const action = `/connect-claude/authorize?mcp_request_id=${encodeURIComponent(mcpRequestId)}`;
  const body = `<section class="card" aria-labelledby="consent-heading">
  <h1 id="consent-heading">Connect ${name} to Distant Signal</h1>
  <p>${name} wants to use your Distant Signal account to look up train departures, arrivals, and journeys on your behalf.</p>
  <p class="dimmed">Only approve this if you started connecting from Claude yourself. Denying sends you back without connecting.</p>
  <form method="POST" action="${escapeHtml(action)}">
    <div class="actions">
      <button type="submit" name="decision" value="approve" class="button button-filled">Approve</button>
      <button type="submit" name="decision" value="deny" class="button button-default">Deny</button>
    </div>
  </form>
</section>`;
  return htmlResponse(renderDocument({ title: `Connect ${name}`, nonce, body }), 200);
}

/** A user-facing error from this route, keeping its HTTP status. `message`
 * is fixed copy from route.ts, never request-derived; it is escaped anyway. */
export function renderErrorPage({
  status,
  heading,
  message,
  nonce,
}: {
  status: number;
  heading: string;
  message: string;
  nonce: string | null;
}): NextResponse {
  const body = `<section class="card" aria-labelledby="error-heading">
  <h1 id="error-heading">${escapeHtml(heading)}</h1>
  <div class="alert" role="alert">${ERROR_ICON}<p>${escapeHtml(message)}</p></div>
  <div class="links">
    <a href="/connect-claude">How to connect Claude</a>
    <a href="/">Back to the home page</a>
  </div>
</section>`;
  return htmlResponse(renderDocument({ title: heading, nonce, body }), status);
}
