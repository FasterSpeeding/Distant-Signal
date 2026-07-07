'use client';

import { Stack, Title, Text, Button } from '@mantine/core';

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Couldn&apos;t load status data</Title>
      <Text c="dimmed">{error.message}</Text>
      <Button onClick={reset} w="fit-content">
        Try again
      </Button>
    </Stack>
  );
}
