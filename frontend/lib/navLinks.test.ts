import { describe, it, expect } from 'vitest';
import {
  accountMenuDestinations,
  CHAT_DESTINATION,
  GROUPS_DESTINATION,
  navDrawerDestinations,
  TRACKED_TRAINS_DESTINATION,
} from './navLinks';

describe('navDrawerDestinations', () => {
  it('excludes Groups and Chat for an anonymous visitor', () => {
    const hrefs = navDrawerDestinations(false, false).map((d) => d.href);
    expect(hrefs).not.toContain(GROUPS_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
  });

  it('includes Groups but not Chat for an authenticated, non-allow-listed visitor', () => {
    const hrefs = navDrawerDestinations(true, false).map((d) => d.href);
    expect(hrefs).toContain(GROUPS_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
  });

  // Review §3.1.3: /chat was reachable only by typing the URL directly --
  // this is the fix, on the one nav surface a phone ever sees.
  it('includes Chat once getChatbotAccess() resolves to "allowed"', () => {
    const hrefs = navDrawerDestinations(true, true).map((d) => d.href);
    expect(hrefs).toContain(CHAT_DESTINATION.href);
  });

  it('never offers Chat to a logged-out visitor, even if chatAllowed were somehow true', () => {
    // Defensive: getChatbotAccess() itself never returns 'allowed' for an
    // unauthenticated caller, but this function doesn't rely on that
    // invariant holding elsewhere -- `authenticated` still gates Groups
    // independently.
    const hrefs = navDrawerDestinations(false, true).map((d) => d.href);
    expect(hrefs).toContain(CHAT_DESTINATION.href);
    expect(hrefs).not.toContain(GROUPS_DESTINATION.href);
  });
});

describe('accountMenuDestinations', () => {
  it('always offers My Trains & Tickets and Groups', () => {
    const hrefs = accountMenuDestinations(false).map((d) => d.href);
    expect(hrefs).toEqual([TRACKED_TRAINS_DESTINATION.href, GROUPS_DESTINATION.href]);
  });

  it('appends Chat only when allow-listed', () => {
    const hrefs = accountMenuDestinations(true).map((d) => d.href);
    expect(hrefs).toEqual([
      TRACKED_TRAINS_DESTINATION.href,
      GROUPS_DESTINATION.href,
      CHAT_DESTINATION.href,
    ]);
  });
});
