import { Alert, Stack, Text } from '@mantine/core';
import {
  cancellationHeadline,
  cancellationReasonPhrase,
  cancellationReasonSource,
  cancelledSectionLines,
} from '@/lib/cancellation';
import type { Cancellation } from '@/lib/types';

/** The red notice for a cancelled or part-cancelled train (DS-181): what
 * no longer runs as the title ("Cancelled", "Terminates at Raynes Park",
 * "Cancelled between X and Y"), each cancelled section when that adds
 * anything, and the reason with where it came from. All of it is text in
 * the Alert, so a screen reader hears the whole sentence and nothing rests
 * on the red alone. */
export function CancellationNotice({ cancellation }: { cancellation: Cancellation }) {
  const headline = cancellationHeadline(cancellation);
  const sections = cancelledSectionLines(cancellation).filter((line) => line !== headline);
  const reason = cancellationReasonPhrase(cancellation.reason);
  const source = cancellationReasonSource(cancellation.reason);
  return (
    <Alert color="red" title={headline} data-cancellation-kind={cancellation.kind}>
      <Stack gap={4}>
        {sections.map((line) => (
          <Text key={line} size="sm">
            {line}.
          </Text>
        ))}
        {reason ? (
          <Text size="sm">Reason: {reason}.</Text>
        ) : cancellation.kind === 'full' ? (
          <Text size="sm">This service was cancelled. No reason has been given.</Text>
        ) : (
          <Text size="sm">No reason has been given.</Text>
        )}
        {source && (
          <Text size="xs" c="dimmed">
            {source}
          </Text>
        )}
      </Stack>
    </Alert>
  );
}
