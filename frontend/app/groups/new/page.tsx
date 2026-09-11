import { Stack, Title } from '@mantine/core';
import { CreateGroupForm } from '@/components/CreateGroupForm';

export default function NewGroupPage() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Create a group</Title>
      <CreateGroupForm />
    </Stack>
  );
}
