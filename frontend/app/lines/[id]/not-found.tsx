import { Stack, Title, Text } from '@mantine/core';

export default function LineNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Line not found</Title>
      <Text c="dimmed">No line matches that ID.</Text>
    </Stack>
  );
}
