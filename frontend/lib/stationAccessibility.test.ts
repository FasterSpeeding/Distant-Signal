import { describe, it, expect } from 'vitest';
import {
  ACCESSIBILITY_CATEGORIES,
  containsMarkup,
  formatDays,
  formatHours,
  hasRenderableValue,
  humanizeKey,
  isEmptyNode,
  isSentence,
  MAX_RENDER_DEPTH,
  renderAccessibilityValue,
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
    expect(node.fields.map((field) => field.label)).toEqual(['Number of spaces', 'Cctv']);
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
      'toiletsAndChanging',
      'lifts',
      'loungesAndWaiting',
      'platformFacilities',
      'stationFacilities',
      'helpAndSupport',
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
