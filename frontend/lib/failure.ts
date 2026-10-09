/**
 * The one way the app words a failed action (docs/style-guide.md,
 * "Writing"): "Couldn't <verb> <noun>." plus what to do next, never a raw
 * response body, a status code or an exception message.
 *
 *   describeFailure('load', 'this train', 500)
 *     -> "Couldn't load this train. Try again."
 */

/** What the visitor can do about each status, after "Couldn't …". */
function nextStep(status: number | undefined): string {
  switch (status) {
    case 401:
      return 'Log in and try again.';
    case 403:
      return "You don't have access.";
    case 404:
    case 410:
      return 'It may have been removed.';
    case 409:
      return 'It changed in the meantime. Reload and try again.';
    case 429:
      return 'Too many requests. Wait a minute and try again.';
    default:
      return 'Try again.';
  }
}

/** "Couldn't <verb> <noun>. <next step>" for an HTTP `status`, or for a
 * network failure when `status` is undefined. `noun` carries its own
 * article: "this train", "your journeys", "the invite link". */
export function describeFailure(verb: string, noun: string, status?: number): string {
  return `Couldn't ${verb} ${noun}. ${nextStep(status)}`;
}

/** The longest API message shown as is. Longer bodies are not a sentence
 * written for a person. */
const MAX_API_MESSAGE_LENGTH = 240;

/** Whether `body` is one of the API's own messages written for people
 * ("You're already tracking 100 upcoming trains, which is the maximum.",
 * "the group owner can't be demoted"), rather than a framework error, an
 * HTML page, JSON or a code such as "not_admin". */
export function isUserFacingApiMessage(body: string): boolean {
  const text = body.trim();
  return text.length > 0 && text.length <= MAX_API_MESSAGE_LENGTH && /^[A-Za-z]/.test(text) && !/[\n<>{}`_]/.test(text);
}

/** An API message as a sentence: capital first letter, closing full stop. */
export function asSentence(text: string): string {
  const trimmed = text.trim();
  const capitalised = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.?!]$/.test(capitalised) ? capitalised : `${capitalised}.`;
}

/** Statuses whose body can be the API telling the visitor what to change. */
const CLIENT_ERROR_STATUSES = new Set([400, 403, 409, 422]);

/** The API's own sentence for a refusal the visitor can act on (a 400,
 * 403, 409 or 422 with a readable body), or null. Reads the body, so call
 * it at most once per response. */
export async function apiRefusalMessage(response: Response): Promise<string | null> {
  if (!CLIENT_ERROR_STATUSES.has(response.status)) return null;
  const body = await response.text().catch(() => '');
  return isUserFacingApiMessage(body) ? asSentence(body) : null;
}

/** The message for a failed `response`: the API's own sentence when it
 * wrote one (`apiRefusalMessage`), otherwise `describeFailure`. Reads the
 * body, so call it at most once per response. */
export async function failureFromResponse(verb: string, noun: string, response: Response): Promise<string> {
  return (await apiRefusalMessage(response)) ?? describeFailure(verb, noun, response.status);
}
