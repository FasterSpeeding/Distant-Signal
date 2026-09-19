// @vitest-environment node
//
// Deliberately NOT jsdom (this repo's default, see vitest.config.ts). jsdom
// always provides `window`, so `GroupInviteLinkCard.test.tsx` -- and any
// other test in this suite -- structurally CANNOT reproduce the bug this
// file guards against: `GroupInviteLinkCard` is a `'use client'` component
// rendered by the async Server Component `app/groups/[id]/page.tsx`, so
// Next.js renders it on the server for the initial HTML, where `window` is
// undefined.
//
// Historically (review §2.11's finding) this component read
// `window.location.origin` in a mount effect: fine on the server (the read
// was deferred out of the render body precisely to avoid a `window`
// `ReferenceError` there), but it meant the server-rendered HTML carried
// only a bare, uncopyable relative path, and `share()` was inert until that
// effect flushed client-side. The fix moved the origin resolution
// server-side entirely (`lib/siteOrigin.ts`'s `getSiteOrigin()`, called by
// `app/groups/[id]/page.tsx`) and threads it down as an ordinary `origin`
// prop -- so this file's job now is proving the ABSOLUTE url is already
// present in the server-rendered HTML, with no client-side fill-in step
// left to reproduce.
//
// `renderToString` in a `node` environment is that server, near enough: no
// `window`, no `document`, and effects never run -- exactly the conditions
// the real SSR pass imposes. `npm run build` does not cover this, because
// `/groups/[id]` is a dynamic (`ƒ`) route that is server-rendered per
// request rather than prerendered at build time, so a build can succeed
// with a `window` read fully intact.
import { describe, it, expect, vi } from 'vitest';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { GroupInviteLinkCard } from './GroupInviteLinkCard';

const ORIGIN = 'https://distant-signal.example';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn(), push: vi.fn() }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Renders on the "server": no `window`, no effect flush. */
function serverRender(ui: React.ReactNode) {
  return renderToString(<MantineProvider theme={theme}>{ui}</MantineProvider>);
}

describe('GroupInviteLinkCard (server render)', () => {
  it('has no window at all in this environment', () => {
    // Guards the guard: if some future setup step started defining
    // `window` here, every assertion below would silently stop testing
    // anything.
    expect(typeof window).toBe('undefined');
  });

  it('server-renders an active invite link without touching window', () => {
    expect(() =>
      serverRender(
        <GroupInviteLinkCard
          groupId="grp-1"
          inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
          origin={ORIGIN}
        />,
      ),
    ).not.toThrow();
  });

  it('server-renders the no-link case without touching window', () => {
    expect(() =>
      serverRender(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} origin={ORIGIN} />),
    ).not.toThrow();
  });

  it('emits the full absolute URL in the server HTML -- no relative-path / client-fill-in step to wait for', () => {
    const html = serverRender(
      <GroupInviteLinkCard
        groupId="grp-1"
        inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }}
        origin={ORIGIN}
      />,
    );
    expect(html).toContain(`${ORIGIN}/groups/join/tok123`);
  });
});
