import { describe, expect, it } from 'vitest';
import {
  ACCOUNT_DESTINATION,
  accountMenuDestinations,
  CHAT_DESTINATION,
  GROUPS_DESTINATION,
  isActiveNavHref,
  navDrawerDestinations,
  PLAN_JOURNEY_DESTINATION,
  PREFERENCES_DESTINATION,
  PRIMARY_NAV_DESTINATIONS,
  TRACK_DESTINATION,
  TRACK_JOURNEY_HREF,
  TRANSIT_DESTINATION,
} from './navLinks';

describe('TRANSIT_DESTINATION', () => {
  it('is in the drawer only while a network is served', () => {
    expect(navDrawerDestinations(false, false).map((d) => d.href)).not.toContain('/transit');
    expect(navDrawerDestinations(true, true, false).map((d) => d.href)).not.toContain('/transit');
  });

  it('sits straight after Lines, and never in the bar', () => {
    const hrefs = navDrawerDestinations(false, false, true).map((d) => d.href);
    expect(hrefs[hrefs.indexOf('/lines') + 1]).toBe(TRANSIT_DESTINATION.href);
    expect(TRANSIT_DESTINATION.label).toBe('Trams');
    expect(PRIMARY_NAV_DESTINATIONS).not.toContain(TRANSIT_DESTINATION);
  });
});

describe('the bar', () => {
  it('has one Track entry, first, then Plan a journey, then the browse pages', () => {
    expect(PRIMARY_NAV_DESTINATIONS.map((d) => d.label)).toEqual([
      'Track',
      'Plan a journey',
      'Status',
      'Lines',
      'Operators',
      'Stations',
      'Incidents',
    ]);
    expect(PRIMARY_NAV_DESTINATIONS[0]).toBe(TRACK_DESTINATION);
    expect(PRIMARY_NAV_DESTINATIONS[1]).toBe(PLAN_JOURNEY_DESTINATION);
  });

  it('sends Track to the Track page, and starts new journeys on /journeys/new', () => {
    expect(TRACK_DESTINATION.href).toBe('/track/mine');
    expect(TRACK_JOURNEY_HREF).toBe('/journeys/new');
  });

  it('lists no tracking page twice', () => {
    const hrefs = PRIMARY_NAV_DESTINATIONS.map((d) => d.href);
    for (const href of ['/trains', '/journeys/new', '/track']) {
      expect(hrefs).not.toContain(href);
    }
  });
});

describe('isActiveNavHref', () => {
  it('matches a plain href exactly, never as a prefix', () => {
    expect(isActiveNavHref('/plan', PLAN_JOURNEY_DESTINATION)).toBe(true);
    expect(isActiveNavHref('/journeys/new', '/plan')).toBe(false);
    expect(isActiveNavHref('/lines/abc', '/lines')).toBe(false);
    expect(isActiveNavHref('/lines', '/')).toBe(false);
    expect(isActiveNavHref(null, '/plan')).toBe(false);
  });

  it('marks Track current on every tracking page', () => {
    for (const path of [
      '/track/mine',
      '/track',
      '/track/mine/add-ticket',
      '/trains',
      '/journeys/new',
      '/journeys/42',
      '/journeys/templates',
    ]) {
      expect(isActiveNavHref(path, TRACK_DESTINATION), path).toBe(true);
    }
    expect(isActiveNavHref('/trainsx', TRACK_DESTINATION)).toBe(false);
    expect(isActiveNavHref('/plan', TRACK_DESTINATION)).toBe(false);
    expect(isActiveNavHref('/train/W12345/2026-09-17', TRACK_DESTINATION)).toBe(false);
  });

  it('marks Status current on the network history page', () => {
    const status = PRIMARY_NAV_DESTINATIONS.find((d) => d.href === '/status')!;
    expect(isActiveNavHref('/network/history', status)).toBe(true);
  });
});

describe('navDrawerDestinations', () => {
  it('carries every primary destination and Preferences for anyone', () => {
    const hrefs = navDrawerDestinations(false, false).map((d) => d.href);
    expect(hrefs).toEqual([...PRIMARY_NAV_DESTINATIONS.map((d) => d.href), PREFERENCES_DESTINATION.href]);
  });

  it('omits Groups, Chat and Account for a logged-out visitor', () => {
    const hrefs = navDrawerDestinations(false, false).map((d) => d.href);
    expect(hrefs).not.toContain(GROUPS_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
    expect(hrefs).not.toContain(ACCOUNT_DESTINATION.href);
  });

  it('adds Groups and Account when logged in, with Preferences last', () => {
    const hrefs = navDrawerDestinations(true, false).map((d) => d.href);
    expect(hrefs).toContain(GROUPS_DESTINATION.href);
    expect(hrefs).toContain(ACCOUNT_DESTINATION.href);
    expect(hrefs).not.toContain(CHAT_DESTINATION.href);
    expect(hrefs.at(-1)).toBe(PREFERENCES_DESTINATION.href);
  });

  it('includes Chat once getChatbotAccess() resolves to "allowed"', () => {
    expect(navDrawerDestinations(true, true).map((d) => d.href)).toContain(CHAT_DESTINATION.href);
  });
});

describe('accountMenuDestinations', () => {
  it('offers Groups, Account and Preferences', () => {
    expect(accountMenuDestinations(false).map((d) => d.href)).toEqual([
      GROUPS_DESTINATION.href,
      ACCOUNT_DESTINATION.href,
      PREFERENCES_DESTINATION.href,
    ]);
  });

  it('adds Chat only when allow-listed', () => {
    expect(accountMenuDestinations(true).map((d) => d.href)).toEqual([
      GROUPS_DESTINATION.href,
      CHAT_DESTINATION.href,
      ACCOUNT_DESTINATION.href,
      PREFERENCES_DESTINATION.href,
    ]);
  });

  it('no longer carries the tracking page, which is in the bar for everyone', () => {
    expect(accountMenuDestinations(true).map((d) => d.href)).not.toContain(TRACK_DESTINATION.href);
  });
});
