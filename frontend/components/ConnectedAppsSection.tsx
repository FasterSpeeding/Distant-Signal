import { ConnectedApps } from './ConnectedApps';
import { getConnectedApps } from '@/lib/api';

/** `ConnectedApps` with its data, for any server-rendered page: the
 * `/account/connected-apps` route, and the Preferences page. Renders
 * nothing for a visitor who is not logged in. */
export async function ConnectedAppsSection({
  headingOrder = 2,
  showTitle = true,
}: {
  headingOrder?: 2 | 3;
  showTitle?: boolean;
}) {
  const data = await getConnectedApps();
  if (!data) return null;
  return <ConnectedApps data={data} headingOrder={headingOrder} showTitle={showTitle} />;
}
