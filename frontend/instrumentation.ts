import type { Instrumentation } from 'next';
import { createLogger } from '@/lib/logger';

const log = createLogger('next');

/** Server startup: switch the Node server's console output to the JSON
 * log format (`lib/consoleBridge.ts`). The Edge runtime has no stdout to
 * bridge. */
export async function register(): Promise<void> {
  if (process.env.NEXT_RUNTIME === 'nodejs') {
    const { installConsoleBridge } = await import('@/lib/consoleBridge');
    installConsoleBridge();
  }
}

/** Every uncaught server error (render, route handler, server action,
 * proxy), as one structured line with the route it hit. The query string
 * is dropped: it can carry share/invite tokens. */
export const onRequestError: Instrumentation.onRequestError = async (error, request, context) => {
  log.error('unhandled error handling a request', {
    error,
    method: request.method,
    path: request.path.split('?')[0],
    route_path: context.routePath,
    route_type: context.routeType,
    router_kind: context.routerKind,
    render_source: context.renderSource,
  });
};
