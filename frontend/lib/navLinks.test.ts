import { describe, expect, it } from 'vitest';
import {
  ACCOUNT_DESTINATION,
  accountMenuDestinations,
  CHAT_DESTINATION,
  GROUPS_DESTINATION,
  isActiveNavHref,
  navDrawerDestinations,
  PLAN_JOURNEY_HREF,
  PLAN_JOURNEY_LABEL,
  PREFERENCES_DESTINATION,
  PRIMARY_NAV_DESTINATIONS,
  JOURNEYS_DESTINATION,
  TRANSIT_DESTINATION,
} from './navLinks';

describe('TRANSIT_DESTINATION', () => {
  it('is in the drawer only while a network is served', () => {
    expect(navDrawerDestinations(false).map((d) => d.href)).not.toContain('/transit');
    expect(navDrawerDestinations(true, false).map((d) => d.href)).not.toContain('/transit');
  });

  it('sits straight after Lines, and never in the bar', () => {
    const hrefs = navDrawerDestinations(false, true).map((d) => d.href);
    expect(hrefs[hrefs.indexOf('/lines') + 1]).toBe(TRANSIT_DESTINATION.href);
    expect(TRANSIT_DESTINATION.label).toBe('Trams');
    expect(PRIMARY_NAV_DESTINATIONS).not.toContain(TRANSIT_DESTINATION);
  });
});

describe('the bar', () => {
  it('has one Journeys entry, first, then the browse pages', () => {
    expect(PRIMARY_NAV_DESTINATIONS.map((d) => d.label)).toEqual([
      'Journeys',
      'Status',
      'Lines',
      'Operators',
      'Stations',
      'Incidents',
    ]);
    expect(PRIMARY_NAV_DESTINATIONS[0]).toBe(JOURNEYS_DESTINATION);
  });

  it('sends Journeys to /journeys, and starts new journeys on /plan, "Plan a journey"', () => {
    expect(JOURNEYS_DESTINATION.href).toBe('/journeys');
    expect(PLAN_JOURNEY_HREF).toBe('/plan');
    expect(PLAN_JOURNEY_LABEL).toBe('Plan a journey');
  });

  it('lists no tracking page twice', () => {
    const hrefs = PRIMARY_NAV_DESTINATIONS.map((d) => d.href);
    for (const href of ['/trains', '/journeys/new', '/track', '/track/mine']) {
      expect(hrefs).not.toContain(href);
    }
  });

  // DS-115, DS-194: a journey starts from the Journeys page and the home
  // page's "Upcoming journeys", never from the nav.
  it('offers no Plan a journey or new-journey entry in the bar, the drawer or the account menu', () => {
    for (const list of [
      PRIMARY_NAV_DESTINATIONS,
      navDrawerDestinations(true, true),
      navDrawerDestinations(false),
      accountMenuDestinations(),
    ]) {
      expect(list.map((d) => d.href)).not.toContain(PLAN_JOURNEY_HREF);
      expect(list.map((d) => d.href)).not.toContain('/journeys/new');
      expect(list.map((d) => d.label)).not.toContain(PLAN_JOURNEY_LABEL);
      expect(list.map((d) => d.label)).not.toContain('New journey');
      expect(list.map((d) => d.label)).not.toContain('Track a journey');
    }
  });
});

describe('isActiveNavHref', () => {
  it('matches a plain href exactly, never as a prefix', () => {
    expect(isActiveNavHref('/plan', '/plan')).toBe(true);
    expect(isActiveNavHref('/plan/x', '/plan')).toBe(false);
    expect(isActiveNavHref('/lines/abc', '/lines')).toBe(false);
    expect(isActiveNavHref('/lines', '/')).toBe(false);
    expect(isActiveNavHref(null, '/plan')).toBe(false);
  });

  it('marks Journeys current on every tracking page, Plan a journey included', () => {
    for (const path of [
      '/journeys',
      '/plan',
      '/track/mine',
      '/track',
      '/track/mine/add-ticket',
      '/trains',
      '/journeys/new',
      '/journeys/42',
      '/journeys/templates',
    ]) {
      expect(isActiveNavHref(path, JOURNEYS_DESTINATION), path).toBe(true);
    }
    expect(isActiveNavHref('/trainsx', JOURNEYS_DESTINATION)).toBe(false);
    expect(isActiveNavHref('/planx', JOURNEYS_DESTINATION)).toBe(false);
    expect(isActiveNavHref('/train/W12345/2026-09-17', JOURNEYS_DESTINATION)).toBe(false);
  });

  it('marks Status current on the network history page', () => {
    const status = PRIMARY_NAV_DESTINATIONS.find((d) => d.href === '/status')!;
    expect(isActiveNavHref('/network/history', status)).toBe(true);
  });
});

describe('navDrawerDestinations', () => {
  it('carries every primary destination and Preferences for anyone', () => {
    const hrefs = navDrawerDestinations(false).map((d) => d.href);
    expect(hrefs).toEqual([...PRIMARY_NAV_DESTINATIONS.map((d) => d.href), PREFERENCES_DESTINATION.href]);
  });

  it('omits Groups, Chat and Account for a logged-out visitor', () => {
    const hrefs = navDrawerDestinations(false).map((d) => d.href);
    expect(hrefs).not.toContain(GROUPS_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
    expect(hrefs).not.toContain(ACCOUNT_DESTINATION.href);
  });

  // Every logged-in visitor gets Chat: it has no access group any more.
  it('adds Groups, Chat and Account when logged in, with Preferences last', () => {
    const hrefs = navDrawerDestinations(true).map((d) => d.href);
    expect(hrefs).toContain(GROUPS_DESTINATION.href);
    expect(hrefs).toContain(CHAT_DESTINATION.href);
    expect(hrefs).toContain(ACCOUNT_DESTINATION.href);
    expect(hrefs.at(-1)).toBe(PREFERENCES_DESTINATION.href);
  });
});

describe('accountMenuDestinations', () => {
  it('offers Groups, Chat, Account and Preferences', () => {
    expect(accountMenuDestinations().map((d) => d.href)).toEqual([
      GROUPS_DESTINATION.href,
      CHAT_DESTINATION.href,
      ACCOUNT_DESTINATION.href,
      PREFERENCES_DESTINATION.href,
    ]);
  });

  it('no longer carries the tracking page, which is in the bar for everyone', () => {
    expect(accountMenuDestinations().map((d) => d.href)).not.toContain(JOURNEYS_DESTINATION.href);
  });
});
