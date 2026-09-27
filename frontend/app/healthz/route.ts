/** Liveness/readiness endpoint for the frontend Deployment's probes (FE-1,
 * 2026-09-27 review). Deliberately dependency-free: no `api` call, no
 * cookies, no session. The probes used to hit `/`, whose server render makes
 * 5-7 calls to `api` -- so whenever `api` was down (every node reboot, while
 * it waits on Postgres and runs migrations) the frontend failed its liveness
 * probe and was killed, then sat in CrashLoopBackOff for minutes after `api`
 * had recovered. This answers "is the Next.js server process serving
 * requests", which is all a liveness probe should ask. */
export const dynamic = 'force-dynamic';

export function GET(): Response {
  return new Response('ok', {
    status: 200,
    headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' },
  });
}
