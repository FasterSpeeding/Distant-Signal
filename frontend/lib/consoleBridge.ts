/**
 * Routes the server's `console.*` output through the JSON logger
 * (`lib/logger.ts`), so Next.js's own messages and errors (which it prints
 * with `console.error`, stack and all) and any library's console output
 * come out as single-line JSON too, with target `console`.
 *
 * Node runtime only: `instrumentation.ts` imports this inside its
 * `NEXT_RUNTIME === 'nodejs'` branch, so `node:util` never reaches another
 * bundle. Not installed under `LOG_FORMAT=pretty`.
 */
import { format } from 'node:util';
import { emit, jsonLoggingActive, originalConsole, type LogFields, type LogLevel } from './logger';

// ANSI SGR sequences (Next's coloured `⨯`/`⚠` prefixes when it thinks it
// has a TTY): never in a JSON line.
const ANSI = /\u001b\[[0-9;]*m/g;

let installed = false;

/** The line one `console.<method>(...args)` call becomes: the first `Error`
 * argument supplies `error`/`stack`, everything else is formatted into
 * `message` as `console` itself would (`util.format`). */
export function consoleCallToLine(args: unknown[]): { message: string; fields: LogFields } {
  const error = args.find((arg) => arg instanceof Error);
  const rest = error === undefined ? args : args.filter((arg) => arg !== error);
  let message = rest.length > 0 ? format(...rest) : '';
  message = message.replace(ANSI, '').trim();
  if (message === '' && error instanceof Error) message = error.message;
  return { message, fields: error === undefined ? {} : { error } };
}

export function installConsoleBridge(): void {
  if (installed || !jsonLoggingActive()) return;
  installed = true;
  const bridge =
    (level: LogLevel) =>
    (...args: unknown[]): void => {
      const { message, fields } = consoleCallToLine(args);
      emit(level, 'console', message, fields, originalConsole);
    };
  console.log = bridge('INFO');
  console.info = bridge('INFO');
  console.debug = bridge('DEBUG');
  console.warn = bridge('WARN');
  console.error = bridge('ERROR');
}
