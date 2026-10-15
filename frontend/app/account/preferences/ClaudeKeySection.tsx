import { ClaudeKeySettings } from '@/components/ClaudeKeySettings';
import { PreferencesSection } from './PreferencesSection';

/** The visitor's Claude API key for "Ask about trains". The key itself is
 * saved in this browser, but it is only any use to an account that can
 * ask about trains, so this section shows only to those. */
export function ClaudeKeySection() {
  return (
    <PreferencesSection
      id="claude-key-heading"
      title="Ask about trains"
      description="Uses Claude with your own API key."
    >
      <ClaudeKeySettings />
    </PreferencesSection>
  );
}
