import { describe, expect, it } from 'vitest';
import { asSentence, describeFailure, failureFromResponse, isUserFacingApiMessage } from './failure';

describe('describeFailure', () => {
  it('reads "Couldn\'t <verb> <noun>. Try again." by default', () => {
    expect(describeFailure('load', 'this train')).toBe("Couldn't load this train. Try again.");
    expect(describeFailure('load', 'this train', 500)).toBe("Couldn't load this train. Try again.");
  });

  it('says what to do for the statuses a visitor can act on', () => {
    expect(describeFailure('save', 'this ticket', 401)).toBe("Couldn't save this ticket. Log in and try again.");
    expect(describeFailure('delete', 'this group', 403)).toBe("Couldn't delete this group. You don't have access.");
    expect(describeFailure('load', 'this journey', 404)).toBe("Couldn't load this journey. It may have been removed.");
    expect(describeFailure('rename', 'this train', 409)).toBe(
      "Couldn't rename this train. It changed in the meantime. Reload and try again.",
    );
    expect(describeFailure('search for', 'a train', 429)).toBe(
      "Couldn't search for a train. Too many requests. Wait a minute and try again.",
    );
  });
});

describe('isUserFacingApiMessage', () => {
  it('accepts the API sentences written for people', () => {
    expect(isUserFacingApiMessage("You're already tracking 100 upcoming trains, which is the maximum.")).toBe(true);
    expect(isUserFacingApiMessage("the group owner can't be demoted")).toBe(true);
  });

  it('rejects codes, markup, JSON and empty bodies', () => {
    expect(isUserFacingApiMessage('not_admin')).toBe(false);
    expect(isUserFacingApiMessage('<html><body>502 Bad Gateway</body></html>')).toBe(false);
    expect(isUserFacingApiMessage('{"error":"x"}')).toBe(false);
    expect(isUserFacingApiMessage('')).toBe(false);
    expect(isUserFacingApiMessage('x'.repeat(300))).toBe(false);
  });
});

describe('asSentence', () => {
  it('capitalises and closes the sentence', () => {
    expect(asSentence("the group owner can't be demoted")).toBe("The group owner can't be demoted.");
    expect(asSentence('Already done.')).toBe('Already done.');
  });
});

describe('failureFromResponse', () => {
  it('shows the API sentence for a refusal the visitor can act on', async () => {
    const response = new Response('that member is already an admin or the owner', { status: 409 });
    expect(await failureFromResponse('change', "this member's role", response)).toBe(
      'That member is already an admin or the owner.',
    );
  });

  it('never shows a server error body', async () => {
    const response = new Response('thread panicked at src/main.rs', { status: 500 });
    expect(await failureFromResponse('save', 'this line', response)).toBe("Couldn't save this line. Try again.");
  });

  it('falls back when a refusal has no usable body', async () => {
    const response = new Response('', { status: 400 });
    expect(await failureFromResponse('save', 'this line', response)).toBe("Couldn't save this line. Try again.");
  });
});
