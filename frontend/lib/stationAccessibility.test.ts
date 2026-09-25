import { describe, it, expect } from 'vitest';
import {
  ACCESSIBILITY_CATEGORIES,
  computeAtAGlance,
  containsMarkup,
  dedupeAcrossSection,
  formatDays,
  formatHours,
  hasRenderableValue,
  hostLabel,
  humanizeKey,
  isEmptyNode,
  isSentence,
  MAX_RENDER_DEPTH,
  renderAccessibilityValue,
  sortEntriesByKind,
  type AccessibilityNode,
} from './stationAccessibility';

/** Narrows to one node kind, failing the test (rather than silently
 * skipping assertions inside an `if`) when the dispatcher picked a
 * different pattern. Every "does this real payload match Pattern X?" test
 * below goes through this, so a regression shows up as "expected 'raw' to
 * be 'facility'", not as a green test that asserted nothing. */
function expectKind<K extends AccessibilityNode['kind']>(
  node: AccessibilityNode,
  kind: K,
): Extract<AccessibilityNode, { kind: K }> {
  expect(node.kind).toBe(kind);
  return node as Extract<AccessibilityNode, { kind: K }>;
}

describe('humanizeKey', () => {
  it('splits camelCase into title-cased words', () => {
    expect(humanizeKey('stepFreeAccess')).toBe('Step free access');
    expect(humanizeKey('lifts')).toBe('Lifts');
    expect(humanizeKey('helpAndSupport')).toBe('Help and support');
  });

  it('splits an acronym run from the word that follows it', () => {
    expect(humanizeKey('hasWCFacilities')).toBe('Has wc facilities');
  });

  it('returns the key unchanged rather than throwing on a key with no word boundaries', () => {
    expect(humanizeKey('')).toBe('');
    expect(humanizeKey('_')).toBe('_');
  });
});

describe('hasRenderableValue', () => {
  it('treats null and undefined as absent, everything else as present', () => {
    expect(hasRenderableValue(null)).toBe(false);
    expect(hasRenderableValue(undefined)).toBe(false);
    expect(hasRenderableValue(false)).toBe(true);
    expect(hasRenderableValue(0)).toBe(true);
    expect(hasRenderableValue('')).toBe(true);
    expect(hasRenderableValue({})).toBe(true);
  });
});

describe('containsMarkup', () => {
  // The six entities the survey found, plus the tags.
  it('detects the tags and entities the feed actually carries', () => {
    expect(containsMarkup('<p>Speak to on train staff.</p>')).toBe(true);
    expect(containsMarkup('<a href="mailto:x@y.z" title="">x@y.z</a>')).toBe(true);
    expect(containsMarkup('ramps can&#39;t be deployed')).toBe(true);
    expect(containsMarkup('Fares &amp; tickets')).toBe(true);
    expect(containsMarkup('&#160;')).toBe(true);
  });

  // ABD's `operatorName`. Not markup, and §4.7 requires it to be escaped
  // by React rather than handed to a sanitizer.
  it('does not treat a bare ampersand or a comparison as markup', () => {
    expect(containsMarkup('Monday - Saturday 07:30 - 21:30 & Sunday 09:00 - 21:00')).toBe(false);
    expect(containsMarkup('platforms 1 < 2 and 3 > 4')).toBe(false);
    expect(containsMarkup('There are lifts')).toBe(false);
  });
});

describe('isSentence', () => {
  // The two cases §4.6 shows a plain ~20-character heuristic gets exactly
  // backwards.
  it('accepts the short self-describing prose a length threshold would reject', () => {
    expect(isSentence('There are lifts')).toBe(true);
    expect(isSentence('There are no lifts')).toBe(true);
  });

  it('accepts real depth-1 feed sentences', () => {
    expect(isSentence('There are tactile warnings on all platforms in use')).toBe(true);
    expect(isSentence('Announcements are made both visually and audibly')).toBe(true);
  });

  it('rejects codes, bare labels and numbers', () => {
    expect(isSentence('SWA')).toBe(false);
    expect(isSentence('CA1 1QZ')).toBe(false);
    expect(isSentence('£6.00')).toBe(false);
    expect(isSentence('-')).toBe(false);
    expect(isSentence('')).toBe(false);
  });
});

describe('Pattern A -- facility record', () => {
  // DNO (Dunrobin Castle), verbatim: the design's own named example of an
  // `available: false` record with explanatory notes.
  const dnoTrainRamp = {
    available: false,
    location: '<p>Speak to on train staff.</p>',
    notes: "<p>Due to low platform constraints ramps can&#39;t be deployed.</p>",
    openingHoursNotes: null,
    openingTimes: null,
    operatorContactDetails: null,
    storage: 'Ramps are available on the train to provide assistance',
  };

  it('matches on a boolean `available` and keeps `false` as a rendered fact', () => {
    const node = expectKind(renderAccessibilityValue(dnoTrainRamp), 'facility');
    expect(node.available).toBe(false);
    // "Not available" is content, never something the empty-skipping hides.
    expect(isEmptyNode(node)).toBe(false);
  });

  it('renders location and notes as unlabelled sanitized rich text, in that order', () => {
    const node = expectKind(renderAccessibilityValue(dnoTrainRamp), 'facility');
    expect(node.parts[0].label).toBeUndefined();
    expect(expectKind(node.parts[0].node, 'richText').html).toContain('Speak to on train staff.');
    expect(expectKind(node.parts[1].node, 'richText').html).toContain('ramps can');
    // The entity is decoded by the sanitizer's own parse, not left literal.
    expect(expectKind(node.parts[1].node, 'richText').html).not.toContain('&#39;');
  });

  it('renders an extra scalar sibling as an unlabelled Pattern E sentence', () => {
    const node = expectKind(renderAccessibilityValue(dnoTrainRamp), 'facility');
    const storage = node.parts.find(
      (part) => part.node.kind === 'sentence' && part.node.text.startsWith('Ramps are available'),
    );
    expect(storage).toBeDefined();
    expect(storage?.label).toBeUndefined();
  });

  it('drops every null sibling rather than printing an empty row', () => {
    const node = expectKind(renderAccessibilityValue(dnoTrainRamp), 'facility');
    expect(node.parts).toHaveLength(3);
  });

  // `transportLinks.*`'s two-field variant -- 155 instances.
  it('handles the {available, notes} narrow variant as the same pattern', () => {
    const node = expectKind(
      renderAccessibilityValue({ available: true, notes: 'There is taxi provision' }),
      'facility',
    );
    expect(node.available).toBe(true);
    expect(expectKind(node.parts[0].node, 'sentence').text).toBe('There is taxi provision');
  });

  it('routes an extra array sibling through Patterns F and D', () => {
    const node = expectKind(
      renderAccessibilityValue({
        available: true,
        names: ['Ticket barriers', 'Ticket barriers, main concourse'],
        locations: [{ name: 'Toilet', radarKeyAvailable: null }],
      }),
      'facility',
    );
    expect(expectKind(node.parts[0].node, 'tokens').tokens).toEqual([
      'Ticket barriers',
      'Ticket barriers, main concourse',
    ]);
    expect(expectKind(node.parts[1].node, 'collection').items[0].label).toBe('Toilet');
  });
});

describe('Pattern B -- opening times', () => {
  // ABD, verbatim.
  const abd = [
    {
      daysOfTheWeek: ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
      openPeriod: [{ endTime: '00:45:00.000', startTime: '05:00:00.000' }],
      openingStatus: 'Specific Hours',
    },
  ];

  it('compacts a day run and trims HH:MM:SS.mmm to HH:MM', () => {
    const node = expectKind(renderAccessibilityValue(abd), 'openingTimes');
    expect(node.entries).toEqual([{ days: 'Mon–Sat', hours: '05:00–00:45' }]);
  });

  // BTN's fully-reversed set. Compacting over array order would say
  // "Sun–Mon".
  it('sorts into week order before compacting, whatever order the feed sends', () => {
    expect(
      formatDays(['Sunday', 'Saturday', 'Friday', 'Thursday', 'Wednesday', 'Tuesday', 'Monday']),
    ).toBe('Mon–Sun');
    // INV, interleaved.
    expect(
      formatDays(['Monday', 'Tuesday', 'Thursday', 'Friday', 'Saturday', 'Wednesday']),
    ).toBe('Mon–Sat');
  });

  it('emits Public Holidays as its own trailing token, never folded into a range', () => {
    expect(formatDays(['Monday', 'Tuesday', 'Public Holidays'])).toBe('Mon–Tue, Public Holidays');
    expect(formatDays(['Public Holidays'])).toBe('Public Holidays');
  });

  it('keeps a non-consecutive day set as separate tokens', () => {
    expect(formatDays(['Monday', 'Wednesday', 'Friday'])).toBe('Mon, Wed, Fri');
    expect(formatDays(['Saturday', 'Sunday'])).toBe('Sat–Sun');
  });

  it('renders the three observed statuses', () => {
    expect(formatHours({ openingStatus: '24 Hours', openPeriod: [] })).toBe('24 hours');
    expect(formatHours({ openingStatus: '24 Hours', openPeriod: null })).toBe('24 hours');
    expect(formatHours({ openingStatus: 'Unavailable', openPeriod: null })).toBe('closed');
  });

  // LLE's two `staffHelp.openingTimes` entries. The record contradicts
  // itself; §4.3 forbids resolving that by deletion.
  it('shows both when a 24 Hours entry also carries a real period', () => {
    expect(
      formatHours({
        openingStatus: '24 Hours',
        openPeriod: [{ startTime: '06:10:00.000', endTime: '12:40:00.000' }],
      }),
    ).toBe('24 hours (source also lists 06:10–12:40)');
  });

  // §9.4: the three values are sample-derived, not documented.
  it('prints an unrecognised status verbatim instead of swallowing it', () => {
    expect(formatHours({ openingStatus: 'Seasonal', openPeriod: null })).toBe('Seasonal');
    expect(
      formatHours({
        openingStatus: 'Seasonal',
        openPeriod: [{ startTime: '09:00:00.000', endTime: '17:00:00.000' }],
      }),
    ).toBe('Seasonal (09:00–17:00)');
  });

  it('does not claim hours it was not given', () => {
    expect(formatHours({ openingStatus: 'Specific Hours', openPeriod: [] })).toBe(
      'hours not published',
    );
    // No status at all and no period: nothing to say, and nothing invented.
    expect(formatHours({})).toBe('');
  });

  // §9.4 again: the eight day tokens are sample-derived too, so an
  // unrecognised one must survive rather than vanish from the line.
  it('passes an unrecognised day token through instead of dropping it', () => {
    expect(formatDays(['Monday', 'Christmas Day'])).toBe('Mon, Christmas Day');
    expect(formatDays(['Christmas Day', 'Christmas Day'])).toBe('Christmas Day');
    expect(formatDays('Monday')).toBe('');
    expect(formatDays(null)).toBe('');
  });

  it('passes a time through unchanged when it is not HH:MM:SS.mmm', () => {
    expect(
      formatHours({
        openingStatus: 'Specific Hours',
        openPeriod: [{ startTime: 'dawn', endTime: 'dusk' }],
      }),
    ).toBe('dawn–dusk');
  });

  it('renders a one-sided period rather than discarding the half it has', () => {
    expect(
      formatHours({ openingStatus: 'Specific Hours', openPeriod: [{ startTime: '09:00:00.000' }] }),
    ).toBe('from 09:00');
    expect(
      formatHours({ openingStatus: 'Specific Hours', openPeriod: [{ endTime: '17:00:00.000' }] }),
    ).toBe('until 17:00');
  });

  it('dumps an opening-times array that sits deeper than the bound allows', () => {
    // Pattern B reads three levels below its own array without going back
    // through the dispatcher, so it has to check the bound for them itself.
    const entry = [{ daysOfTheWeek: ['Monday'], openPeriod: [], openingStatus: '24 Hours' }];
    // Array at depth 4 -> its period objects at 7, the last allowed level.
    expect(
      JSON.stringify(renderAccessibilityValue({ a: { b: { c: { d: entry } } } })),
    ).not.toContain('"raw"');
    // One level deeper and the period objects would fall outside it.
    expect(
      JSON.stringify(renderAccessibilityValue({ a: { b: { c: { d: { e: entry } } } } })),
    ).toContain('"raw"');
  });
});

describe('Pattern C -- contact details', () => {
  // GLQ's refreshments tenant, verbatim -- §4.4's one apparent exception to
  // "every `name` is redundant", which turns out to duplicate its own
  // `operatorName`.
  const greggs = {
    emailAddress: null,
    name: 'GREGGS BAKERY',
    note: '<p>U6 Queen Street Station</p><p>North Hanover Street</p><p>G1 2AF</p>',
    operatorName: 'GREGGS BAKERY',
    primaryTelephoneNumber: null,
  };

  it('matches on the presence of primaryTelephoneNumber, even when it is null', () => {
    expectKind(renderAccessibilityValue(greggs), 'contact');
  });

  it('drops `name` and keeps `operatorName`', () => {
    const node = expectKind(renderAccessibilityValue(greggs), 'contact');
    expect(node.fields.map((field) => field.label)).toEqual(['Operator', 'Note']);
    expect(expectKind(node.fields[0].node, 'text').text).toBe('GREGGS BAKERY');
  });

  it('links a phone number, an email and a url', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: '0345 077 4224',
        emailAddress: 'customer.relations@scotrail.co.uk',
        url: 'http://www.apcoa.co.uk',
        name: 'Birmingham New Street Car Park 1 Contact Details',
        note: null,
        operatorName: 'APCOA Parking (UK) Limited',
        postalAddress: null,
      }),
      'contact',
    );
    expect(expectKind(node.fields[0].node, 'link').href).toBe('tel:03450774224');
    expect(expectKind(node.fields[1].node, 'link').href).toBe(
      'mailto:customer.relations@scotrail.co.uk',
    );
    // `http:` must survive -- 45 of the sample's 193 anchors use it.
    const website = expectKind(node.fields[2].node, 'link');
    expect(website.href).toBe('http://www.apcoa.co.uk');
    expect(website.external).toBe(true);
  });

  it('joins a postal address into one line, skipping nulls and dash placeholders', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: null,
        postalAddress: {
          addressLine1: 'Court Square',
          addressLine2: 'Carlisle',
          addressLine3: null,
          addressLine4: null,
          addressLine5: null,
          postcode: 'CA1 1QZ',
        },
      }),
      'contact',
    );
    expect(expectKind(node.fields[0].node, 'text').text).toBe('Court Square, Carlisle, CA1 1QZ');
  });

  it('drops an address of nothing but "-" placeholders rather than printing "-, -"', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: null,
        postalAddress: { addressLine1: '-', addressLine2: '-', postcode: null },
      }),
      'contact',
    );
    expect(node.fields).toHaveLength(0);
    expect(isEmptyNode(node)).toBe(true);
  });

  // ABD/DNO/INV's ScotRail lost-property contact, and CDF/LLE's Transport
  // for Wales one: `operatorName` is a bare anchor at five sites (§4.7).
  it('treats an operatorName that is a bare anchor as rich text', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: null,
        operatorName: '<a href="https://www.scotrail.co.uk/contact">ScotRail</a>',
      }),
      'contact',
    );
    expect(expectKind(node.fields[0].node, 'richText').html).toContain('href="https://');
  });
});

describe('Pattern D -- named-item collection', () => {
  // BAL's platform 3, verbatim -- §2.3's own example of the bullet branch.
  const balPlatform = {
    helpPointClose: 'There is a Help Point close to this platform',
    name: 'Platform 3',
    seatingAtIntervals: 'Seating is limited on this platform',
    waitingType: null,
  };

  it('renders an all-string item as unlabelled bullets under its name', () => {
    const node = expectKind(renderAccessibilityValue([balPlatform]), 'collection');
    expect(node.items[0].label).toBe('Platform 3');
    const bullets = expectKind(node.items[0].body, 'bullets');
    expect(bullets.items.map((item) => (item.kind === 'sentence' ? item.text : null))).toEqual([
      'There is a Help Point close to this platform',
      'Seating is limited on this platform',
    ]);
  });

  // §4.1's one real precedence overlap: these items satisfy BOTH D's array
  // predicate and A's object predicate, and D is the right answer.
  it('wins over Pattern A for passengerAssistance items, which carry both name and available', () => {
    const node = expectKind(
      renderAccessibilityValue([
        {
          available: true,
          location: '<p>Assistance Meeting Point</p>',
          name: 'Balham Passenger Assistance Meeting Point 0',
          notes: '<p>Station road entrance ticket barriers.</p>',
        },
      ]),
      'collection',
    );
    expect(node.items[0].label).toBe('Balham Passenger Assistance Meeting Point 0');
    // The structured branch, because `available` is not a string -- and
    // `available` comes out as an ordinary labelled boolean line (§4.5).
    const { fields } = expectKind(node.items[0].body, 'fields');
    const availability = fields.find((field) => field.label === 'Available');
    expect(availability).toBeDefined();
    expect(expectKind(availability!.node, 'text').text).toBe('Yes');
  });

  it('recurses the structured branch into Patterns B, C and D again', () => {
    const node = expectKind(
      renderAccessibilityValue([
        {
          name: 'Car park 1',
          numberOfSpaces: 120,
          charges: { dailyRate: '£6.00', annualRate: null },
          openingHours: [
            {
              daysOfTheWeek: ['Saturday'],
              openPeriod: [],
              openingStatus: '24 Hours',
            },
          ],
          operator: {
            contactDetails: {
              primaryTelephoneNumber: '0345 077 4224',
              name: 'Car Park 1 Contact Details',
            },
          },
          accessibleLocations: [{ name: 'Bay 1', accessibilityInfo: { helpPointClose: 'There is no Help Point close to the accessible parking' } }],
        },
      ]),
      'collection',
    );
    const { fields } = expectKind(node.items[0].body, 'fields');
    const byLabel = new Map(fields.map((field) => [field.label, field.node]));
    expect(expectKind(byLabel.get('Number of spaces')!, 'text').text).toBe('120');
    // The `charges` rate object -- one of §4.9's unmatched interior shapes
    // -- lands on the labelled key/value branch, with its null rate gone.
    const charges = expectKind(byLabel.get('Charges')!, 'fields');
    expect(charges.fields.map((field) => field.label)).toEqual(['Daily rate']);
    expect(expectKind(byLabel.get('Opening hours')!, 'openingTimes').entries).toEqual([
      { days: 'Sat', hours: '24 hours' },
    ]);
    // `operator` is the PARENT of a Pattern C object, not one itself.
    const operator = expectKind(byLabel.get('Operator')!, 'fields');
    expectKind(operator.fields[0].node, 'contact');
    expectKind(byLabel.get('Accessible locations')!, 'collection');
  });

  // §4.5's two "fit the bullet branch but read badly there" exceptions.
  it('renders a {name, crsCode} item as "Swansea (SWA)" linked to that station', () => {
    const node = expectKind(
      renderAccessibilityValue([{ crsCode: 'SWA', name: 'Swansea' }]),
      'collection',
    );
    expect(node.items[0].label).toBe('Swansea (SWA)');
    expect(node.items[0].link).toEqual({ href: '/stations/SWA', external: false });
  });

  it('renders a {name, url} map item as a link with the name as its text', () => {
    const node = expectKind(
      renderAccessibilityValue([
        { name: 'Rail Replacement Bus Map - Balham 1', url: 'https://assets.nationalrail.co.uk/x.pdf' },
      ]),
      'collection',
    );
    expect(node.items[0].label).toBe('Rail Replacement Bus Map - Balham 1');
    expect(node.items[0].link).toEqual({
      href: 'https://assets.nationalrail.co.uk/x.pdf',
      external: true,
    });
  });

  it('falls back to a plain text row when a phone number has no digits at all', () => {
    const node = expectKind(
      renderAccessibilityValue({ primaryTelephoneNumber: 'see website' }),
      'contact',
    );
    expectKind(node.fields[0].node, 'text');
  });

  it('is not matched by an array whose elements lack a string name', () => {
    // `openPeriod`, reached outside Pattern B, must not be read as a
    // collection.
    expectKind(renderAccessibilityValue([{ startTime: '05:00:00.000' }]), 'list');
  });
});

describe('Patterns E and F', () => {
  it('drops the label from a sentence-valued scalar in the fallback branch', () => {
    const node = expectKind(
      renderAccessibilityValue({
        tactilePaving: 'There are tactile warnings on all platforms in use',
      }),
      'fields',
    );
    expect(node.fields[0].label).toBeUndefined();
    expect(expectKind(node.fields[0].node, 'sentence').text).toBe(
      'There are tactile warnings on all platforms in use',
    );
  });

  // §4.6's failure case for a pure length heuristic: capitalised, spaced,
  // 36 characters, and still a code that needs its label.
  it('keeps the label on a code-like field even though it reads like a sentence', () => {
    const node = expectKind(
      renderAccessibilityValue({
        category: 'B1, (refer to quick reference guide)',
        levelAccess: null,
      }),
      'fields',
    );
    expect(node.fields[0].label).toBe('Category');
    expect(expectKind(node.fields[0].node, 'text').text).toBe(
      'B1, (refer to quick reference guide)',
    );
  });

  it('keeps the label on a number or a boolean', () => {
    const node = expectKind(
      renderAccessibilityValue({ numberOfSpaces: 80, cctv: true }),
      'fields',
    );
    // 'cctv' -> 'CCTV': the acronym map review §3.5.11 asks for.
    expect(node.fields.map((field) => field.label)).toEqual(['Number of spaces', 'CCTV']);
    expect(expectKind(node.fields[1].node, 'text').text).toBe('Yes');
  });

  it('humanizes camelCase tokens but leaves prose and acronyms alone', () => {
    const node = expectKind(
      renderAccessibilityValue(['DepartureScreens', 'Announcements', 'Yes - from help point', 'CCTV']),
      'tokens',
    );
    expect(node.tokens).toEqual([
      'Departure screens',
      'Announcements',
      'Yes - from help point',
      'CCTV',
    ]);
  });
});

describe('Pattern G -- rich text', () => {
  it('sanitizes before the node is ever constructed', () => {
    const node = expectKind(
      renderAccessibilityValue('<p onclick="alert(1)">Hi</p><script>alert(2)</script>'),
      'richText',
    );
    expect(node.html).not.toContain('script');
    expect(node.html).not.toContain('onclick');
    expect(node.html).toContain('Hi');
  });

  it('treats a note that is only empty markup as nothing to show', () => {
    expect(isEmptyNode(renderAccessibilityValue('<p></p>'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('<p>&#160;</p>'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('<p>Real copy</p>'))).toBe(false);
  });

  it('treats punctuation-only text and N/A as empty', () => {
    // Lone period or dash
    expect(isEmptyNode(renderAccessibilityValue('.'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('-'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('...'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('--'))).toBe(true);
    // N/A (case-insensitive)
    expect(isEmptyNode(renderAccessibilityValue('N/A'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('n/a'))).toBe(true);
    // Same tests wrapped in markup
    expect(isEmptyNode(renderAccessibilityValue('<p>.</p>'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('<p>-</p>'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('<p>N/A</p>'))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue('<p>n/a</p>'))).toBe(true);
    // But a sentence that merely contains a period should not be empty
    expect(isEmptyNode(renderAccessibilityValue('Not available.'))).toBe(false);
  });
});

describe('fields that arrive as an unexpected type', () => {
  // Design §5 reason 2 names silently dropping a field the worst possible
  // failure mode for accessibility data. A fixed field list that renders
  // `notes` only when it is a string would do exactly that the day the feed
  // sends an array -- no label, no raw block, no trace.
  it('still renders a facility field whose type the bespoke slot does not handle', () => {
    const node = expectKind(
      renderAccessibilityValue({ available: true, notes: ['One note', 'Another'] }),
      'facility',
    );
    expect(node.parts[0].label).toBe('Notes');
    expect(expectKind(node.parts[0].node, 'tokens').tokens).toEqual(['One note', 'Another']);
  });

  it('still renders a contact field whose type the bespoke slot does not handle', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: null,
        postalAddress: 'Court Square, Carlisle',
        emailAddress: 42,
      }),
      'contact',
    );
    expect(node.fields.map((field) => field.label)).toContain('Email address');
    // A string `postalAddress` reads as prose, so Pattern E drops its label
    // -- but the value itself is on the page, which is the property that
    // matters.
    expect(JSON.stringify(node)).toContain('Court Square, Carlisle');
  });

  it('still renders an address line, or a contact name, that is not a string', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: null,
        name: { label: 'Depot contact' },
        postalAddress: { addressLine1: 'Court Square', postcode: 12345 },
      }),
      'contact',
    );
    const rendered = JSON.stringify(node);
    // The string line is joined as usual...
    expect(rendered).toContain('Court Square');
    // ...and neither the numeric postcode nor the object `name` disappears.
    expect(rendered).toContain('12345');
    expect(rendered).toContain('Depot contact');
  });

  it('reads postal address lines in a fixed order, not in JSON key order', () => {
    const node = expectKind(
      renderAccessibilityValue({
        primaryTelephoneNumber: null,
        postalAddress: {
          postcode: 'CA1 1QZ',
          addressLine2: 'Carlisle',
          addressLine1: 'Court Square',
          // Not an address line -- must not be joined in as one.
          country: 'United Kingdom',
        },
      }),
      'contact',
    );
    expect(expectKind(node.fields[0].node, 'text').text).toBe('Court Square, Carlisle, CA1 1QZ');
    // `country` is not an address line this renderer knows, so it is not
    // joined into the line -- but it is still shown, on its own row.
    expect(node.fields).toHaveLength(2);
    expect(JSON.stringify(node.fields[1])).toContain('United Kingdom');
  });
});

describe('the fallback branch', () => {
  // §4.9: the old renderer dumped the WHOLE object the moment one value was
  // nested. That bail is what produced the 96.2%.
  it('renders an unmatched object as labelled rows, recursing rather than bailing to raw', () => {
    const node = expectKind(
      renderAccessibilityValue({ spaces: { notes: null, numberOfSpaces: 80 } }),
      'fields',
    );
    const spaces = expectKind(node.fields[0].node, 'fields');
    expect(spaces.fields[0].label).toBe('Number of spaces');
  });

  it('recurses arbitrarily deep plain objects up to the bound, then dumps', () => {
    // Eight nested containers: depths 0..7, the last of which is the
    // deepest this renderer claims.
    const atBound = { a: { b: { c: { d: { e: { f: { g: { h: 'deep' } } } } } } } };
    expect(JSON.stringify(renderAccessibilityValue(atBound))).not.toContain('"raw"');
    // Nine, so the innermost sits at depth 8 and is dumped instead.
    const pastBound = { a: { b: { c: { d: { e: { f: { g: { h: { i: 'deeper' } } } } } } } } };
    expect(JSON.stringify(renderAccessibilityValue(pastBound))).toContain('"raw"');
  });

  it('states its depth bound in the units the design does', () => {
    // The deepest real chain is seven containers at depths 0-6; the bound
    // is that plus exactly one level of margin.
    expect(MAX_RENDER_DEPTH).toBe(7);
  });
});

describe('renderAccessibilityValue never throws', () => {
  it('handles the scalars', () => {
    expect(expectKind(renderAccessibilityValue(2), 'text').text).toBe('2');
    expect(expectKind(renderAccessibilityValue(true), 'text').text).toBe('Yes');
    expect(expectKind(renderAccessibilityValue(false), 'text').text).toBe('No');
  });

  it('handles the empty containers as "nothing to show", not as a crash', () => {
    expect(isEmptyNode(renderAccessibilityValue({}))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue([]))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue({ notes: null }))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue([{}, {}]))).toBe(true);
    expect(isEmptyNode(renderAccessibilityValue([{ a: 1 }, {}]))).toBe(false);
  });

  it('survives a cycle, which JSON.stringify itself throws on', () => {
    const cyclic: Record<string, unknown> = { self: null };
    cyclic.self = cyclic;
    expect(() => renderAccessibilityValue(cyclic)).not.toThrow();
  });

  it('degrades a non-plain object to raw rather than enumerating its internals', () => {
    expectKind(renderAccessibilityValue(new Date('2026-09-16')), 'raw');
    expectKind(renderAccessibilityValue(new Map([['a', 1]])), 'raw');
  });

  it('degrades null and undefined to raw rather than throwing, even though callers skip them first', () => {
    expect(() => renderAccessibilityValue(null)).not.toThrow();
    expect(() => renderAccessibilityValue(undefined)).not.toThrow();
    expectKind(renderAccessibilityValue(null), 'raw');
    expectKind(renderAccessibilityValue(undefined), 'raw');
  });

  // The raw fallback must never be hidden: it is the only thing standing
  // between an unanticipated shape and silently dropped data.
  it('never flags a raw fallback as empty', () => {
    expect(isEmptyNode(renderAccessibilityValue(null))).toBe(false);
    expect(isEmptyNode(renderAccessibilityValue(new Date()))).toBe(false);
  });
});

describe('ACCESSIBILITY_CATEGORIES', () => {
  it('covers exactly the twelve allowlisted keys, once each, in the spec-defined group order', () => {
    const allKeys = ACCESSIBILITY_CATEGORIES.flatMap((c) => c.keys);
    expect(allKeys).toEqual([
      'stationAccessibility',
      'staffAssistance',
      'helpAndSupport',
      'toiletsAndChanging',
      'lifts',
      'loungesAndWaiting',
      'platformFacilities',
      'stationFacilities',
      'transportLinks',
      'carParks',
      'dropOffPickUp',
      'cycling',
    ]);
    expect(ACCESSIBILITY_CATEGORIES.map((c) => c.heading)).toEqual([
      'Step-free access & assistance',
      'Facilities',
      'Platform & station facilities',
      'Getting here',
    ]);
  });

  // The backend's own ACCESSIBILITY_KEYS const
  // (crates/api/src/data/reference.rs) is asserted against the same twelve
  // names in its own Rust test. This is the frontend half of that pairing:
  // a key added to the wire allowlist but not to a category here would be
  // fetched and then silently never rendered.
  it('matches the backend allowlist as a set, so no forwarded key goes unrendered', () => {
    const backendAllowlist = [
      'stationAccessibility',
      'staffAssistance',
      'toiletsAndChanging',
      'lifts',
      'transportLinks',
      'cycling',
      'carParks',
      'dropOffPickUp',
      'platformFacilities',
      'stationFacilities',
      'helpAndSupport',
      'loungesAndWaiting',
    ];
    expect([...ACCESSIBILITY_CATEGORIES.flatMap((c) => c.keys)].sort()).toEqual(
      [...backendAllowlist].sort(),
    );
  });
});

describe('hostLabel (review §3.5.9)', () => {
  it('strips the scheme and a leading www., appending an outbound arrow', () => {
    expect(hostLabel('https://www.nationalrail.co.uk/stations_destinations/x.aspx')).toBe(
      'nationalrail.co.uk ↗',
    );
  });

  it('keeps a non-www host as-is', () => {
    expect(hostLabel('http://example.com/path')).toBe('example.com ↗');
  });

  it('returns null for a URL it cannot parse, rather than throwing', () => {
    expect(hostLabel('not a url')).toBeNull();
  });
});

describe('dedupeAcrossSection (review §3.5.4)', () => {
  it('drops a later field whose whole rendered content byte-for-byte repeats an earlier one', () => {
    const seen = new Set<string>();
    const first = dedupeAcrossSection(renderAccessibilityValue({ helpline: 'Ring the office' }), seen);
    const second = dedupeAcrossSection(renderAccessibilityValue({ helpline: 'Ring the office' }), seen);
    expect(expectKind(first, 'fields').fields).toHaveLength(1);
    // The exact-duplicate field is gone from the second render, but the
    // node itself survives (not null) so callers keep a stable shape.
    expect(expectKind(second, 'fields').fields).toHaveLength(0);
  });

  it('keeps a field that only partially overlaps an earlier one', () => {
    const seen = new Set<string>();
    dedupeAcrossSection(
      renderAccessibilityValue({ helpPoints: { available: false, notes: 'Same notes' } }),
      seen,
    );
    const second = dedupeAcrossSection(
      renderAccessibilityValue({
        helpPoints: { available: false, notes: 'Same notes', inductionLoop: 'Yes' },
      }),
      seen,
    );
    // `notes` (the duplicate) is gone; `inductionLoop` (new information)
    // survives -- exactly the MAN-fixture "Help points" case this fix
    // targets.
    const facility = expectKind(second, 'fields').fields[0].node;
    expectKind(facility, 'facility');
    expect(isEmptyNode(facility)).toBe(false);
    const partLabels = expectKind(facility, 'facility').parts.map((p) => p.label);
    expect(partLabels).not.toContain(undefined); // the unlabelled `notes` sentence is gone
    expect(partLabels).toContain('Induction loop');
  });

  it('never removes a collection item that merely shares content with a sibling item', () => {
    // The other big source of repeated text in the survey (many platforms
    // sharing "Lift controls should be accessible to most people") must
    // NOT be deduplicated away -- each is a distinct physical item.
    const seen = new Set<string>();
    const node = dedupeAcrossSection(
      renderAccessibilityValue({
        platforms: [
          { name: 'Platform 1', note: 'Same note' },
          { name: 'Platform 2', note: 'Same note' },
        ],
      }),
      seen,
    );
    const collection = expectKind(expectKind(node, 'fields').fields[0].node, 'collection');
    expect(collection.items).toHaveLength(2);
    expect(collection.items.map((item) => item.label)).toEqual(['Platform 1', 'Platform 2']);
  });
});

describe('sortEntriesByKind (review §3.5.3)', () => {
  it('puts a simple text/sentence fact ahead of a large collection, keeping ties in feed order', () => {
    const entries = [
      { id: 'lifts', node: { kind: 'list', items: [] } as AccessibilityNode },
      { id: 'category', node: { kind: 'text', text: 'A' } as AccessibilityNode },
      { id: 'tokens', node: { kind: 'tokens', tokens: [] } as AccessibilityNode },
    ];
    expect(sortEntriesByKind(entries).map((e) => e.id)).toEqual(['category', 'tokens', 'lifts']);
  });
});

describe('computeAtAGlance (review §3.5.3)', () => {
  it('surfaces a station\'s step-free category, lift count and accessible-toilet facts', () => {
    const facts = computeAtAGlance({
      stationAccessibility: { stepFreeCategory: { category: 'A, Compliant step-free access' } },
      lifts: { liftsInfo: [{ name: 'Lift 1' }, { name: 'Lift 2' }] },
      toiletsAndChanging: {
        toilets: { accessibleToiletsAvailable: true, changingPlacesToiletsAvailable: false },
      },
    });
    expect(facts).toContainEqual({ label: 'Step-free category', value: 'A, Compliant step-free access' });
    expect(facts).toContainEqual({ label: 'Lifts', value: '2' });
    expect(facts).toContainEqual({ label: 'Accessible toilet', value: 'Yes' });
    expect(facts.find((f) => f.label === 'Changing Places')).toBeUndefined();
  });

  it('returns no facts at all for a payload with none of the recognised shapes', () => {
    expect(computeAtAGlance({ cycling: 'Racks on the forecourt' })).toEqual([]);
  });

  it('never throws on a malformed shape for any of its fields', () => {
    expect(() =>
      computeAtAGlance({
        stationAccessibility: 'not an object' as never,
        lifts: null as never,
        carParks: [1, 2, 3] as never,
      }),
    ).not.toThrow();
  });
});

describe('review §3.5.8: a facility free-text duplicate of its own contact phone number', () => {
  it('drops the facility\'s notes when they are exactly its contact\'s phone number', () => {
    const node = expectKind(
      renderAccessibilityValue({
        available: true,
        notes: '0345 077 4224',
        operatorContactDetails: { primaryTelephoneNumber: '0345 077 4224' },
      }),
      'facility',
    );
    // The duplicate notes line is gone; the Contact block (with its own
    // tappable tel: link) is still there.
    expect(node.parts.some((p) => p.label === 'Contact')).toBe(true);
    expect(node.parts.some((p) => !p.label)).toBe(false);
  });

  it('keeps a facility\'s notes that merely contain, but are not exactly, the contact phone number', () => {
    const node = expectKind(
      renderAccessibilityValue({
        available: true,
        notes: 'Call reception, not the main helpline 0345 077 4224',
        operatorContactDetails: { primaryTelephoneNumber: '0345 077 4224' },
      }),
      'facility',
    );
    expect(node.parts.some((p) => !p.label)).toBe(true);
  });
});

describe('review §3.5.11: label defects', () => {
  it('does not relabel a nested field with the same name as its own containing key ("Car parks" x2)', () => {
    const node = expectKind(
      renderAccessibilityValue({ accessibleParkingSpacesAvailable: true, carParks: [{ name: 'Long Stay' }] }, 'carParks'),
      'fields',
    );
    // The nested `carParks` array renders unlabelled -- the group entry's
    // own "Car parks" heading (rendered by the caller, not this node)
    // already said it once.
    const collectionField = node.fields.find((f) => f.node.kind === 'collection');
    expect(collectionField?.label).toBeUndefined();
  });

  it('drops the redundant "Category:" label when the parent key already ends in "category"', () => {
    const node = expectKind(
      renderAccessibilityValue({
        stepFreeCategory: { category: 'A, Compliant step-free access to all platforms' },
      }),
      'fields',
    );
    expect(node.fields[0].label).toBe('Step free category');
    const inner = expectKind(node.fields[0].node, 'fields');
    expect(inner.fields.map((f) => f.label)).toEqual([undefined]);
  });

  it('renders "Location" the same way whether it sits in a facility or a plain object', () => {
    // Short enough that it does NOT qualify as a Pattern E sentence (§4.6's
    // 12-character/shape rule) -- exactly the case where a facility's own
    // `location` (always unlabelled by construction) and a Pattern D item's
    // sibling `location` field (previously only unlabelled when long enough
    // to read as a sentence) used to disagree.
    const facilityNode = expectKind(
      renderAccessibilityValue({ available: true, location: 'Concourse' }),
      'facility',
    );
    expect(facilityNode.parts.find((p) => !p.label)?.node).toEqual({ kind: 'text', text: 'Concourse' });

    const plainNode = expectKind(renderAccessibilityValue({ location: 'Concourse' }), 'fields');
    expect(plainNode.fields[0].label).toBeUndefined();
    expect(plainNode.fields[0].node).toEqual({ kind: 'text', text: 'Concourse' });
  });

  it('expands ATM/CCTV/Wi-Fi as acronyms rather than mechanically capitalising the first letter', () => {
    expect(humanizeKey('atm')).toBe('ATM');
    expect(humanizeKey('cctvAvailable')).toBe('CCTV available');
    expect(humanizeKey('wifi')).toBe('Wi-Fi');
  });

  it('relabels feed-CMS field names to words a traveller would use', () => {
    expect(humanizeKey('liftsInfo')).toBe('Lift details');
    expect(humanizeKey('names')).toBe('Named locations');
  });

  // Signal Box Audit, flib Low finding: "prototype-key lookups can render a
  // function as a label". A feed field literally named `constructor` (or
  // another Object.prototype key) used to make the internal
  // `ACRONYM_WORDS[word]` lookup resolve to `Object.prototype.constructor`
  // (a function) instead of `undefined`, which `?? word` would not catch --
  // producing a function where a string label is expected.
  it('treats a field named "constructor" as an ordinary word, not a prototype method', () => {
    expect(humanizeKey('constructor')).toBe('Constructor');
    expect(typeof humanizeKey('constructor')).toBe('string');
  });

  it('treats "constructor" as an ordinary leading word even when it is the first of several', () => {
    // `constructorInfo` splits into the tokens `['constructor', 'info']` --
    // `constructor` lands in the same `ACRONYM_WORDS[firstWord]` lookup the
    // bare-word case above exercises, just as the first of two words rather
    // than the whole key.
    expect(humanizeKey('constructorInfo')).toBe('Constructor info');
  });
});
