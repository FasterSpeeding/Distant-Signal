import { AnalyticsOptIn } from '@/components/AnalyticsOptIn';
import { TextLink } from '@/components/TextLink';
import { PreferencesSection } from './PreferencesSection';

/** "Help improve Distant Signal" (DS-190): the opt-in for anonymous usage
 * counts, shown to everyone. Logged in, the choice is saved to the
 * account; logged out, in this browser (`components/AnalyticsOptIn.tsx`).
 * The same switch is on /cookies. The privacy link shows only while the
 * legal pages render (`legalPagesVisible()`), as the footer's do. */
export function UsageAnalyticsSection({
  signedIn,
  accountOptIn,
  showPrivacyLink,
}: {
  signedIn: boolean;
  accountOptIn: boolean;
  showPrivacyLink: boolean;
}) {
  return (
    <PreferencesSection id="usage-analytics-heading" title="Usage counts">
      <AnalyticsOptIn session={{ signedIn, accountOptIn }} />
      {showPrivacyLink && (
        <TextLink href="/privacy" size="sm">
          How we use this information
        </TextLink>
      )}
    </PreferencesSection>
  );
}
