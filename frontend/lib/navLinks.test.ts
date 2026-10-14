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
  TRANSIT_DESTINATION,
} from './navLinks';

describe('TRANSIT_DESTINATION', () => {
  it('is hidden while no transit network is served (the default)', () => {
    expect(navDrawerDestinations(false).map((d) => d.href)).not.toContain('/transit');
    expect(navDrawerDestinations(true, false).map((d) => d.href)).not.toContain('/transit');
  });

  it('follows Lines in the drawer once a network is served, and never joins the inline bar', () => {
    const hrefs = navDrawerDestinations(false, true).map((d) => d.href);
    expect(hrefs[hrefs.indexOf('/lines') + 1]).toBe(TRANSIT_DESTINATION.href);
    expect(TRANSIT_DESTINATION.label).toBe('Trams');
    expect(PRIMARY_NAV_DESTINATIONS).not.toContain(TRANSIT_DESTINATION);
  });
});

describe('TRACK_JOURNEY_DESTINATION', () => {
  it('points at the /journeys/new creation flow', () => {
    expect(TRACK_JOURNEY_DESTINATION.href).toBe('/journeys/new');
  });

  it('leads PRIMARY_NAV_DESTINATIONS, ahead of Status -- the main way to start tracking something', () => {
    expect(PRIMARY_NAV_DESTINATIONS[0]).toBe(TRACK_JOURNEY_DESTINATION);
  });

  it('is included in the drawer, for every visitor', () => {
    const hrefs = navDrawerDestinations(false).map((d) => d.href);
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
      const drawer = navDrawerDestinations(authenticated);
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
  it('excludes Groups, Chat and the account page for an anonymous visitor', () => {
    const hrefs = navDrawerDestinations(false).map((d) => d.href);
    expect(hrefs).not.toContain(GROUPS_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
    expect(hrefs).not.toContain(ACCOUNT_DESTINATION.href);
  });

  // Review §3.1.3: /chat was reachable only by typing the URL directly --
  // this is the fix, on the one nav surface a phone ever sees. Every
  // logged-in visitor gets it: the chat has no access group any more.
  it('includes Groups, Chat and the account page for every authenticated visitor', () => {
    const hrefs = navDrawerDestinations(true).map((d) => d.href);
    expect(hrefs).toContain(GROUPS_DESTINATION.href);
    expect(hrefs).toContain(CHAT_DESTINATION.href);
    expect(hrefs).toContain(ACCOUNT_DESTINATION.href);
  });
});

describe('accountMenuDestinations', () => {
  it('offers My Trains & Tickets, Groups, Chat and the account page', () => {
    const hrefs = accountMenuDestinations().map((d) => d.href);
    expect(hrefs).toEqual([
      TRACKED_TRAINS_DESTINATION.href,
      GROUPS_DESTINATION.href,
      CHAT_DESTINATION.href,
      ACCOUNT_DESTINATION.href,
    ]);
  });
});
