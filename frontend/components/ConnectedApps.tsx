'use client';

import { Card, Group, List, ListItem, Stack, Text } from '@mantine/core';
import { ButtonLink } from './ButtonLink';
import { ConfirmActionButton } from './ConfirmActionButton';
import { SectionTitle } from './SectionTitle';
import { deletionSummary, scopeSentence } from '@/lib/connectedApps';
import { formatDate, formatDateTime } from '@/lib/dateFormat';
import { trackFeature } from '@/lib/usageAnalytics';
import type { ConnectedAppsData } from '@/lib/types';

/** The Connected apps list (docs/connected-apps.md): each app the visitor
 * connected, what it may do, when it was connected and last used, and a
 * Disconnect button; the apps they could connect; and what an app deleted
 * in the last 30 days, so a journey can be saved again by hand.
 *
 * Self-contained so any page can show it: `ConnectedAppsSection` fetches
 * the data on the server and renders this. It lives at
 * `/account/connected-apps`, and belongs on the Preferences page too. */
export function ConnectedApps({
  data,
  headingOrder = 2,
  showTitle = true,
}: {
  data: ConnectedAppsData;
  /** The level of this component's headings, below the page's own. */
  headingOrder?: 2 | 3;
  /** `false` on `/account/connected-apps`, whose page title already says it. */
  showTitle?: boolean;
}) {
  const connected = new Set(data.grants.map((grant) => grant.client));
  const connectable = data.available.filter((app) => !connected.has(app.name));
  const appName = (client: string) =>
    data.grants.find((grant) => grant.client === client)?.displayName ??
    data.available.find((app) => app.name === client)?.displayName ??
    client;

  return (
    <Stack gap="lg">
      <Stack
        gap="sm"
        component="section"
        {...(showTitle ? { 'aria-labelledby': 'connected-apps-heading' } : { 'aria-label': 'Connected apps' })}
      >
        {showTitle && (
          <SectionTitle order={headingOrder} id="connected-apps-heading">
            Connected apps
          </SectionTitle>
        )}
        <Text size="sm">
          An app you connect can work with your journeys while you use it, for example an assistant saving a trip you
          planned with it. Disconnect it here at any time.
        </Text>
        {data.grants.length === 0 && <Text c="dimmed">No apps are connected.</Text>}
        {data.grants.map((grant) => (
          <Card key={grant.client} withBorder>
            <Stack gap="xs" align="flex-start">
              <Text fw={600}>{grant.displayName}</Text>
              <List size="sm">
                {grant.scopes.map((scope) => (
                  <ListItem key={scope}>{scopeSentence(scope)}</ListItem>
                ))}
              </List>
              <Text size="sm" c="dimmed">
                Connected {formatDateTime(grant.createdAt)}.{' '}
                {grant.lastUsedAt ? `Last used ${formatDateTime(grant.lastUsedAt)}.` : 'Not used yet.'}
              </Text>
              {!grant.clientEnabled && (
                <Text size="sm">Distant Signal has turned it off for now, so it can&apos;t use your account.</Text>
              )}
              <ConfirmActionButton
                label="Disconnect"
                actionName={`disconnect ${grant.displayName}`}
                title={`Disconnect ${grant.displayName}?`}
                request={async () => {
                  const response = await fetch(`/api/account/connected-apps/${encodeURIComponent(grant.client)}`, {
                    method: 'DELETE',
                  });
                  if (response.ok) trackFeature('connected_app', 'revoked');
                  return response;
                }}
                failure={['disconnect', 'the app']}
                loginPrompt="Log in to disconnect it"
              >
                It will stop being able to use your journeys straight away. You can connect it again later.
              </ConfirmActionButton>
            </Stack>
          </Card>
        ))}
        {connectable.length > 0 && (
          <Group gap="sm">
            {connectable.map((app) => (
              <ButtonLink
                key={app.name}
                href={`/account/connected-apps/${encodeURIComponent(app.name)}`}
                variant="light"
              >
                Connect {app.displayName}
              </ButtonLink>
            ))}
          </Group>
        )}
      </Stack>

      <Stack gap="sm" component="section" aria-labelledby="assistant-deletions-heading">
        <SectionTitle order={headingOrder} id="assistant-deletions-heading">
          Recently deleted by the assistant
        </SectionTitle>
        <Text size="sm">
          Journeys a connected app deleted in the last 30 days. To get one back, plan it again and save it.
        </Text>
        {data.recentDeletions.length === 0 && <Text c="dimmed">Nothing in the last 30 days.</Text>}
        {data.recentDeletions.length > 0 && (
          <List spacing="xs">
            {data.recentDeletions.map((deletion) => {
              const { title, legs } = deletionSummary(deletion);
              return (
                <ListItem key={deletion.id}>
                  <Text size="sm" fw={600} span>
                    {title}
                  </Text>
                  <Text size="sm" c="dimmed">
                    Deleted by {appName(deletion.client)} on {formatDate(deletion.deletedAt)}.
                  </Text>
                  {legs.map((leg) => (
                    <Text size="sm" key={leg}>
                      {leg}
                    </Text>
                  ))}
                </ListItem>
              );
            })}
          </List>
        )}
      </Stack>
    </Stack>
  );
}
