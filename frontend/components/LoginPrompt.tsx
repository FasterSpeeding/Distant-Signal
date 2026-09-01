import { TextLink } from './TextLink';

/** The shared "you need to log in to do that" affordance next to a Tier-2
 * control (§Reusable pattern of
 * docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md). `verb`
 * is inserted directly after "Log in to " -- pass a bare verb phrase
 * ("pin", "create a line", "delete a line"), not a full sentence. */
export function LoginPrompt({ verb }: { verb: string }) {
  return (
    <TextLink href="/api/auth/login" underline="always">
      Log in to {verb}
    </TextLink>
  );
}
