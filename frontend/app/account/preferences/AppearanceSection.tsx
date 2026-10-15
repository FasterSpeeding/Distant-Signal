import { PrideModeSelect } from '@/components/PrideMode';
import { ThemePreference } from '@/components/ThemePreference';
import { PreferencesSection } from './PreferencesSection';

/** The colour theme and the pride flag. Device-local: both are saved in
 * this browser (Mantine's colour scheme and `lib/prideMode.ts`), so they
 * work logged out, and logging in or out leaves them as they are. */
export function AppearanceSection() {
  return (
    <PreferencesSection id="appearance-heading" title="Appearance" description="Saved in this browser.">
      <ThemePreference />
      <PrideModeSelect />
    </PreferencesSection>
  );
}
