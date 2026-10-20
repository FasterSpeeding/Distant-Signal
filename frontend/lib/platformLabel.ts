/** A calling point's platform, as `PlatformBadge` words it. */
export interface PlatformFacts {
  platform: string;
  plannedPlatform: string | null;
  platformChanged: boolean;
}

/** Whether the platform has changed from a known planned one. A change
 * with no planned platform to name reads as an ordinary platform. */
export function isPlatformChange({ plannedPlatform, platformChanged }: PlatformFacts): boolean {
  return platformChanged && plannedPlatform !== null;
}

/** The full platform label: "Platform 4", or "Platform 9 (changed from 6)"
 * when it changed. The words a screen reader always hears. */
export function platformLabel(facts: PlatformFacts): string {
  return isPlatformChange(facts)
    ? `Platform ${facts.platform} (changed from ${facts.plannedPlatform})`
    : `Platform ${facts.platform}`;
}

/** The phone form, for a pill under a "Platform" column header: "4", or
 * "9 (was 6)" when it changed. The change is still said in words, never
 * left to the pill's colour. */
export function compactPlatformLabel(facts: PlatformFacts): string {
  return isPlatformChange(facts) ? `${facts.platform} (was ${facts.plannedPlatform})` : facts.platform;
}
