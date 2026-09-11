// @vitest-environment node
//
// Deliberately NOT jsdom (this repo's default, see vitest.config.ts). jsdom
// always provides `window`, so `GroupInviteLinkCard.test.tsx` -- and any
// other test in this suite -- structurally CANNOT reproduce the bug this
// file exists for: `GroupInviteLinkCard` is a `'use client'` component
// rendered by the async Server Component `app/groups/[id]/page.tsx`, so
// Next.js renders it on the server for the initial HTML, where `window` is
// undefined. Reading `window.location.origin` in the render body threw a
// `ReferenceError` there for every admin/owner of every group -- including
// immediately after `CreateGroupForm` mints the first invite link and
// navigates straight to this page.
//
// `renderToString` in a `node` environment is that server, near enough: no
// `window`, no `document`, and effects never run -- exactly the conditions
// the real SSR pass imposes. `npm run build` does not cover this, because
// `/groups/[id]` is a dynamic (`ƒ`) route that is server-rendered per
// request rather than prerendered at build time, so a build can succeed
// with this crash fully intact.
import { describe, it, expect, vi } from 'vitest';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { GroupInviteLinkCard } from './GroupInviteLinkCard';

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
        <GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />,
      ),
    ).not.toThrow();
  });

  it('server-renders the no-link case without touching window', () => {
    expect(() => serverRender(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} />)).not.toThrow();
  });

  it('emits the token in the server HTML, as a relative URL until the client fills in the origin', () => {
    const html = serverRender(
      <GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />,
    );
    expect(html).toContain('/groups/join/tok123');
  });
});
