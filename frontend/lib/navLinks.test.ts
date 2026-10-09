import { describe, it, expect } from 'vitest';
import {
  ACCOUNT_DESTINATION,
  accountMenuDestinations,
  CHAT_DESTINATION,
  GROUPS_DESTINATION,
  isActiveNavHref,
  navDrawerDestinations,
  PLAN_JOURNEY_DESTINATION,
  PRIMARY_NAV_DESTINATIONS,
  TRACK_JOURNEY_DESTINATION,
  TRACKED_TRAINS_DESTINATION,
} from './navLinks';

describe('TRACK_JOURNEY_DESTINATION', () => {
  it('points at the /journeys/new creation flow', () => {
    expect(TRACK_JOURNEY_DESTINATION.href).toBe('/journeys/new');
  });

  it('leads PRIMARY_NAV_DESTINATIONS, ahead of Status -- the main way to start tracking something', () => {
    expect(PRIMARY_NAV_DESTINATIONS[0]).toBe(TRACK_JOURNEY_DESTINATION);
  });

  it('is included in the drawer, for every visitor', () => {
    const hrefs = navDrawerDestinations(false, false).map((d) => d.href);
    expect(hrefs).toContain(TRACK_JOURNEY_DESTINATION.href);
  });
});

describe('PLAN_JOURNEY_DESTINATION', () => {
  it('points at the /plan page', () => {
    expect(PLAN_JOURNEY_DESTINATION).toEqual({ href: '/plan', label: 'Plan a journey' });
  });

  it('is not one of the always-inline primary links: the bar adds it itself, from lg up', () => {
    expect(PRIMARY_NAV_DESTINATIONS).not.toContain(PLAN_JOURNEY_DESTINATION);
  });

  it('sits straight after "Track a Journey" in the drawer, signed in or not', () => {
    for (const authenticated of [false, true]) {
      const drawer = navDrawerDestinations(authenticated, false);
      const trackIndex = drawer.indexOf(TRACK_JOURNEY_DESTINATION);
      expect(trackIndex).toBeGreaterThanOrEqual(0);
      expect(drawer[trackIndex + 1]).toBe(PLAN_JOURNEY_DESTINATION);
      expect(drawer.filter((d) => d.href === '/plan')).toHaveLength(1);
    }
  });
});

describe('isActiveNavHref', () => {
  it('matches the exact path only', () => {
    expect(isActiveNavHref('/plan', '/plan')).toBe(true);
    expect(isActiveNavHref('/journeys/new', '/plan')).toBe(false);
    expect(isActiveNavHref('/lines/abc', '/lines')).toBe(false);
    expect(isActiveNavHref('/lines', '/')).toBe(false);
    expect(isActiveNavHref(null, '/plan')).toBe(false);
  });
});

describe('navDrawerDestinations', () => {
  it('excludes Groups and Chat for an anonymous visitor', () => {
    const hrefs = navDrawerDestinations(false, false).map((d) => d.href);
    expect(hrefs).not.toContain(GROUPS_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
    expect(hrefs).not.toContain(ACCOUNT_DESTINATION.href);
  });

  it('includes the account page for an authenticated visitor', () => {
    expect(navDrawerDestinations(true, false).map((d) => d.href)).toContain(ACCOUNT_DESTINATION.href);
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
    expect(hrefs).toEqual([TRACKED_TRAINS_DESTINATION.href, GROUPS_DESTINATION.href, ACCOUNT_DESTINATION.href]);
  });

  it('appends Chat only when allow-listed', () => {
    const hrefs = accountMenuDestinations(true).map((d) => d.href);
    expect(hrefs).toEqual([
      TRACKED_TRAINS_DESTINATION.href,
      GROUPS_DESTINATION.href,
      CHAT_DESTINATION.href,
      ACCOUNT_DESTINATION.href,
    ]);
  });
});
