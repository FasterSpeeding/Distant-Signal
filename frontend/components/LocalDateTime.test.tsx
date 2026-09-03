import { describe, it, expect, vi, afterEach } from 'vitest';
import { act } from 'react';
import { renderToString } from 'react-dom/server';
import { hydrateRoot } from 'react-dom/client';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { renderWithMantine } from '@/test/render';
import { LocalDateTime } from './LocalDateTime';

// 18:56 UTC on 19 Aug 2026 is 19:56 the same evening in London (BST) but
// 03:56 the *next morning* in Tokyo -- a different time and a different
// date, so every assertion below distinguishes the two zones on both
// fields rather than on a one-hour offset that a formatter bug could
// coincidentally reproduce.
const INSTANT = '2026-08-19T18:56:01Z';
const IN_LONDON = '19 Aug 2026, 19:56';
const IN_TOKYO = '20 Aug 2026, 03:56';

const originalTz = process.env.TZ;

afterEach(() => {
  process.env.TZ = originalTz;
});

/** Hydrates `element` onto `serverHtml` and reports every channel React 19
 * could complain through.
 *
 * Which channel that is was determined empirically on this repo's
 * react-dom 19.2, not assumed: with a custom `onRecoverableError` supplied,
 * a text mismatch arrives *only* there -- `console.error` and
 * `console.warn` stay completely silent, because a custom handler replaces
 * React's own default logging. Drop the handler and the same failure
 * instead lands on `console.error` (jsdom has no `reportError` for React's
 * default to prefer). So both are captured: `onRecoverableError` is the
 * channel that actually fires here, and the console spies catch anything
 * React logs outside it. The `'harness'` test below is the negative control
 * proving this plumbing detects a real mismatch -- without it, "no errors
 * were reported" would be indistinguishable from "nothing was listening." */
async function hydrateAndCollect(serverHtml: string, element: React.ReactNode) {
  const previousActEnvironment = (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT;
  // Otherwise React itself console.errors "The current testing environment
  // is not configured to support act(...)", which would trip the
  // console.error assertion for a reason that has nothing to do with
  // hydration. `renderWithMantine` gets this for free from
  // @testing-library/react; a raw `hydrateRoot` does not.
  (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

  const container = document.createElement('div');
  container.innerHTML = serverHtml;
  document.body.appendChild(container);

  const consoleErrors: string[] = [];
  const consoleWarns: string[] = [];
  const recoverableErrors: string[] = [];
  const errorSpy = vi
    .spyOn(console, 'error')
    .mockImplementation((...args) => void consoleErrors.push(String(args[0])));
  const warnSpy = vi
    .spyOn(console, 'warn')
    .mockImplementation((...args) => void consoleWarns.push(String(args[0])));

  let root: ReturnType<typeof hydrateRoot> | undefined;
  try {
    await act(async () => {
      root = hydrateRoot(container, element, {
        onRecoverableError: (error) => {
          recoverableErrors.push(String((error as Error)?.message ?? error));
        },
      });
    });
  } finally {
    errorSpy.mockRestore();
    warnSpy.mockRestore();
  }

  const text = container.textContent ?? '';
  act(() => root?.unmount());
  container.remove();
  (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = previousActEnvironment;

  return { consoleErrors, consoleWarns, recoverableErrors, text };
}

describe('LocalDateTime', () => {
  it('server-rendered output is the London time, never the rendering process\'s own zone', () => {
    // Mirrors LastUpdated.test.tsx's regression check: renderToString never
    // runs effects, so this is exactly what the server sends down. TZ is
    // forced to Tokyo to stand in for a server whose ambient zone is not
    // the viewer's -- in production that is the container's UTC
    // (frontend/Dockerfile sets no TZ). If someone deletes the
    // `useMounted()` gate, the host-zone format leaks into the server's
    // markup and this fails.
    process.env.TZ = 'Asia/Tokyo';
    const html = renderToString(
      <MantineProvider theme={theme}>
        <LocalDateTime value={INSTANT} />
      </MantineProvider>,
    );
    expect(html).toContain(IN_LONDON);
    expect(html).not.toContain(IN_TOKYO);
  });

  it('shows the host-local time once mounted', () => {
    process.env.TZ = 'Asia/Tokyo';
    // Asserted on `container.textContent` rather than `screen.getByText`
    // because the component renders a bare fragment -- there is no element
    // wrapping just this text, and the nearest one also holds
    // MantineProvider's injected <style> blocks.
    const { container } = renderWithMantine(<LocalDateTime value={INSTANT} />);
    expect(container.textContent).toContain(IN_TOKYO);
    expect(container.textContent).not.toContain(IN_LONDON);
  });

  it('hydrates London server markup in a Tokyo browser with no mismatch, then switches to local', async () => {
    // The whole point of the component, end to end: markup produced by a
    // process in one zone, hydrated by a process in another. React only
    // diffs against the server's output on the *first* client render, and
    // `useMounted()` is still false then, so both sides render the
    // London-pinned string and agree; the local format only lands on the
    // effect pass that follows.
    process.env.TZ = 'Europe/London';
    const serverHtml = renderToString(
      <MantineProvider theme={theme}>
        <LocalDateTime value={INSTANT} />
      </MantineProvider>,
    );
    expect(serverHtml).toContain(IN_LONDON);

    process.env.TZ = 'Asia/Tokyo';
    const { consoleErrors, consoleWarns, recoverableErrors, text } = await hydrateAndCollect(
      serverHtml,
      <MantineProvider theme={theme}>
        <LocalDateTime value={INSTANT} />
      </MantineProvider>,
    );

    expect(recoverableErrors).toEqual([]);
    expect(consoleErrors).toEqual([]);
    expect(consoleWarns).toEqual([]);
    expect(text).toContain(IN_TOKYO);
    expect(text).not.toContain(IN_LONDON);
  });

  it('harness: the same plumbing does report a genuine server/client text mismatch', async () => {
    // Negative control for the test above. `hydrateAndCollect` asserting
    // "nothing was reported" is only worth anything if it can report
    // something, and React's channel for this has moved between versions
    // (19 routes recoverable hydration errors through `onRecoverableError`,
    // not a console warning). This fails loudly if a future React, or a
    // change to this file's spies, quietly stops surfacing mismatches.
    function Divergent({ text }: { text: string }) {
      return <span>{text}</span>;
    }
    const serverHtml = renderToString(<Divergent text="server-only text" />);
    const { recoverableErrors } = await hydrateAndCollect(
      serverHtml,
      <Divergent text="client-only text" />,
    );
    expect(recoverableErrors).toHaveLength(1);
    expect(recoverableErrors[0]).toContain('Hydration failed');
  });
});
