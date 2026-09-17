import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import type { StationAccessibilityData } from '@/lib/types';

const FIXTURE_DIR = path.join(__dirname, 'accessibility');

/** The 31 real station payloads the structured-rendering design was derived
 * from, one file per CRS.
 *
 * Provenance, per
 * docs/superpowers/specs/2026-09-16-structured-accessibility-rendering-design.md
 * §1.1-§1.2: each file is the exact object
 * `GET /public/stations/{crs}/accessibility` returned from the public
 * production deployment on 2026-09-16 -- i.e. the post-
 * `filter_accessibility_fields` twelve-key slice, recovered verbatim from
 * the RSC flight payload of `https://ds.cursed.solutions/stations/{crs}`,
 * not a re-derivation. Stored compact; the only edit is dropping the
 * capture tool's `//` provenance header so the files are valid JSON.
 *
 * Sample chosen before any results were seen, to span termini and request
 * stops, all four nations and many operators, and deliberately including
 * the extremes: `MAN` (largest payload), `BAL` (smallest) and `DNO`
 * (a seasonal request stop whose `trainRamp.available` is `false`).
 *
 * Known artifact (§1.3): 22 of the 6,996 strings came back as RSC
 * de-duplication references (`"$2b"`, `"$35"`, ...) rather than their
 * literal text. Those are an artifact of reading the flight payload, not
 * feed values -- they stand in for a string either way, so no shape claim
 * these fixtures support depends on them. Committed here for the same
 * reason `crates/poller-tfl/tests/fixtures/` holds real API captures: a
 * test asserting "the real feed renders through Pattern B" is only worth
 * anything if the real feed is what it is given. */
export const ACCESSIBILITY_FIXTURE_CRS: string[] = readdirSync(FIXTURE_DIR)
  .filter((file) => file.endsWith('.json'))
  .map((file) => file.slice(0, -'.json'.length))
  .sort();

export function loadAccessibilityFixture(crs: string): StationAccessibilityData {
  return JSON.parse(
    readFileSync(path.join(FIXTURE_DIR, `${crs}.json`), 'utf8'),
  ) as StationAccessibilityData;
}

export function loadAllAccessibilityFixtures(): { crs: string; data: StationAccessibilityData }[] {
  return ACCESSIBILITY_FIXTURE_CRS.map((crs) => ({ crs, data: loadAccessibilityFixture(crs) }));
}
