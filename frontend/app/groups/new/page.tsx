import { Stack, Text, Title } from '@mantine/core';
import { CreateGroupForm } from '@/components/CreateGroupForm';

// Review §3.2.6: this page said nothing about what happens after "Create
// group" -- CreateGroupForm.tsx's own doc comment explains that it
// immediately rotates the new group's first invite link (spec §6) before
// navigating to the detail page where GroupInviteLinkCard shows it, but
// none of that was visible to the person filling in the form. Also
// `maw={480}` (review §3.2.8, deliberately on top of Task 1.1's root-cause
// `<main>` fix, not instead of it): a one-field form stretching the full
// content width reads as unfinished, the same "line length" reasoning
// Task 1.1's own note gives for leaving this as a separate Groups-specific
// typography decision.
export default function NewGroupPage() {
  return (
    <Stack p="lg" gap="md" maw={480}>
      <Title order={1}>Create a group</Title>
      <Text c="dimmed">You&apos;ll get an invite link to share as soon as it&apos;s created.</Text>
      <CreateGroupForm />
    </Stack>
  );
}
