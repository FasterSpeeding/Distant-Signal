# CORPUS: the NLC XML delivery format beside the JSON extract — design

Status: implemented (schedule-ingest; `scheduleFeed.corpus.enabled` stays off
by default).
Date: 2026-10-14. Extends
[2026-09-28-corpus-sftp-ingest-design](2026-09-28-corpus-sftp-ingest-design.md).

## Context

Network Rail now offers CORPUS through a second API version. Both versions
can be subscribed to at once, so both are supported side by side and the
migration can be gradual. The new version pushes dated full snapshots to the
same SFTP account: `NLC20261009.xml.gz` holds the whole CORPUS as of that
date. The real 2026-10-09 file (not committed) is 349,226 bytes gzipped and
7,150,852 bytes inflated.

## The format, measured on the real file

```xml
<ns2:TSDBData xmlns:ns2="http://tempuri.org/XMLSchema.xsd" xmlns:ns3="…UICExtract" xmlns:ns4="…TAFTSI/5.2">
  <ns2:TSDBDataItem>
    <ns2:NLCTiplocCode>…</ns2:NLCTiplocCode>
    …
  </ns2:TSDBDataItem>
</ns2:TSDBData>
```

- 12,102 `TSDBDataItem` rows, the only children of the root. There is no
  header, no CDATA, no comments and no attributes, and the text is pure
  ASCII. The only entity is `&amp;` (275 times).
- Fields, always in this order. Two are optional elements: they are absent,
  not blank, on some rows.

| Element | Present | Blank (`" "`) | Maps to | Notes |
| --- | --- | --- | --- | --- |
| `NLCTiplocCode` | 12,102 | 0 | `tiploc` | 4 to 7 chars, all unique |
| `NLCCapts-Ident` | 12,102 | 0 | (ignored) | small integers |
| `NLCLocationCode` | 12,102 | 0 | `nlc` | always 6 digits, all unique |
| `NLCCheckChar` | 12,102 | 0 | (ignored) | one letter |
| `NLCDescription` | 12,102 | 0 | `nlc_desc` | up to 26 chars; 15 have trailing padding |
| `NLCCapriDesc` | 12,102 | 9,275 | `nlc_desc16` | up to 16 chars |
| `NLCCrsCode` | **3,580** | 3 | `crs` | 3 letters, all unique |
| `NLCTiplocOwner` | 12,102 | 2,463 | (ignored) | 2 letters |
| `NLCEuroLocation` | 12,102 | 4,373 | `uic` | 5 digits; equals the JSON `UIC` |
| `NLCStanoxCode` | **11,162** | 0 | `stanox` | 5 digits; 327 STANOXes shared by 2+ TIPLOCs |
| `NLCPoMcpCode` | 12,102 | 0 | (ignored) | mostly `0` |

So there are no duplicate keys: NLC, TIPLOC, CRS and UIC are each unique,
and only STANOX repeats, as it does in the JSON extract. The JSON extract
repeats 20 NLCs and 20 TIPLOCs.

### Against the JSON extract

The comparison is with production's loaded `CORPUSExtract.json.gz` of
2026-10-02, 55,981 rows, of which 12,403 have a TIPLOC. The XML format holds
**only the TIPLOC-bearing locations**: every XML row has a TIPLOC.

- 12,099 TIPLOCs are in both. 284 are only in the JSON (for example `ABJN`
  and `ALTRMET`); 3 are only in the XML.
- On those 12,099 shared TIPLOCs the codes agree: NLC, UIC, STANOX and CRS
  all match, except `GOLBRNE`, which gained a STANOX and CRS in the XML.
- Descriptions differ on 3,843 TIPLOCs. The two versions word names
  differently (`REDBRIDGE HANTS` against `REDBRIDGE`). `NLCCapriDesc` is the
  16-character name, the counterpart of `NLCDESC16`.
- The JSON carries CRS codes on 392 rows that have no TIPLOC, and STANOXes on
  428 such rows. The XML has neither.
- In the runtime crosswalk (`common::corpus_inference`, before the
  `stations` filter), 167 TIPLOC→CRS pairs are only in the JSON (mostly
  tram, bus and platform codes) and 18 only in the XML. No TIPLOC gets a
  different CRS. For STANOX→CRS, 2 pairs are only in the JSON and 1 only in
  the XML. Of the 2,659 TIPLOCs in production's stored, station-filtered
  crosswalk, only `WLSD` is absent from the XML file.

The JSON extract is therefore the superset. Switching to the XML alone would
cost the non-TIPLOC locations and a few crosswalk fills.

## Design

### Parsing (`crates/schedule-ingest/src/corpus_xml.rs`)

- The parse streams. The gzip stream is inflated through a capped reader
  (`CORPUS_MAX_DECOMPRESSED_BYTES`, as for JSON, and the same gzip-bomb guard
  as the transit SIRI-SX parser) straight into quick-xml. The workspace's
  0.41, with no features.
- Elements are matched by local name, so the namespace prefix may change.
- Values are trimmed, and a blank or absent value becomes NULL. An all-digit
  NLC is padded to 6 digits and an all-digit STANOX to 5, as for JSON.
- The file is **rejected** for:
  - a root other than `TSDBData`;
  - any element beside the items;
  - a nested element inside a field;
  - a field that appears twice in one item;
  - text between items;
  - an entity XML does not predefine;
  - a truncated document;
  - a row with no NLC;
  - fewer than `CORPUS_XML_MIN_ROWS` rows (10,000; the real file has 12,102).

  A rejection is counted as `schedule_feed_corpus_rejected_total{format="xml"}`
  (`DistantSignalCorpusRejected`), and the file moves to `corpus/rejected/`.
- An unknown *field* inside an item is ignored with a warning, because a new
  column is no reason to stop loading.

### Selection, ordering and archiving (`corpus.rs`, `pattern.rs`)

- **Selection.** An XML snapshot is any `NLC*.xml.gz` name, matched
  case-insensitively by prefix and extension. This is not a configurable
  glob. It never matches the CIF pattern.
- **Drift.** Any other `NLC*` name, such as `.xml`, `.zip` or `.json.gz`, is
  never loaded, moved or deleted. Instead it produces:
  - a warning, once per file;
  - a count in `schedule_feed_corpus_failures_total{stage="drift"}` on every
    poll while such a file is present (the counter
    `DistantSignalCorpusLoadFailing` reads for its other stages);
  - the current number of such files in
    `schedule_feed_corpus_format_drift_files`;
  - the `DistantSignalCorpusFormatDrift` alert (below).

  This works whenever CORPUS ingest is on, even with the XML format off.
- **Ordering.** Files are ordered by the mtime that the scan observes, with
  the name as a tie-break, never by the date in the name. That date is
  metadata only:
  - it is logged with each load;
  - it is exported as `schedule_feed_corpus_xml_file_date_seconds`;
  - a warning is logged when a snapshot's date is earlier than the newest
    archived snapshot's. The file is still processed in receipt order.
- **Stability.** A decision is made only once *every* CORPUS candidate has
  been stable for `stabilityCycles` scans. This prevents a half-uploaded
  newer file from being passed over for an older one.
- **Archiving.**
  - The loaded file goes to `/data/schedule-feed/corpus/`.
  - A rejected file goes to `corpus/rejected/`.
  - Every other candidate is archived unloaded in `corpus/superseded/`. This
    covers an older file, and the non-preferred format.
  - Each directory keeps `retentionKeep` (3) files.
  - Archived names are `<mtime YYYYMMDDTHHMMSSZ>-<name>`.

  Before 2026-10-14, superseded files went into `corpus/` itself.

### Coexistence

- Both formats load into the same `CorpusLocation` model and the same
  `corpus_locations`/`corpus_deliveries` tables. `source_file` says which
  format was loaded. Each load still replaces the whole table, and
  everything downstream is unchanged: the crosswalk rebuild, the freshness
  gauge, the stale alert and the comparison.
- `CORPUS_FORMAT_PRECEDENCE` (`scheduleFeed.corpus.formatPrecedence`):
  - `json` (default): load the newest JSON extract. An XML snapshot is
    loaded only when no JSON extract has been loaded within
    `CORPUS_PREFERRED_GRACE_DAYS` (35, one monthly cycle plus a few days).
    That age is judged from the newest file in `corpus/`, so it survives
    restarts. Otherwise the snapshot is still parsed, so a broken one is
    rejected and alerts. It is also compared, then archived in
    `superseded/`. This means the XML takes over automatically if NWR
    retires the JSON feed, and loads on a fresh install that only receives
    XML.
  - `xml`: the mirror image.
  - `newest`: the newest file of either format. With both feeds subscribed,
    the table alternates between a 56k-row and a 12k-row set. This mode
    exists for completeness, not as a recommendation.
- **Why `json` is the default.** The JSON extract is the superset. It is
  what production loads today, so deploying this changes nothing until a
  decision is made. And `newest` would make the table flap between formats.
  Both formats are *enabled* by default (`jsonEnabled`, `xml.enabled`)
  because the XML side then gives a free migration signal and a fallback.
- **Comparison** (`corpus_formats.rs`, `CORPUS_COMPARE_FORMATS`, on by
  default; read-only). After each decision, the file is compared with the
  newest file of the other format. That file comes from the landing folder
  if one is there, and otherwise from `corpus/` or `corpus/superseded/`.
  The comparison covers:
  - the TIPLOC coverage of each side;
  - code mismatches on shared TIPLOCs;
  - description mismatches;
  - the crosswalk pairs only one side gives, or gives differently.

  The results are logged with sample keys and exported as
  `schedule_feed_corpus_format_diff{measure}`. `corpus_compare`, which
  compares the loaded CORPUS with the timetable, is unchanged and works on
  whichever format was loaded.
- **Guards.** Every guard applies to both formats: gzip, the decompression
  cap, the per-format minimum rows, the NLC on every row, rejection
  (`DistantSignalCorpusRejected`, now with a `format` label), the
  `(mtime, size)` re-check before every move, `DistantSignalCorpusLoadFailing`
  (scan, read and load failures of either format) and
  `DistantSignalCorpusStale` (warning at 35 days, critical at 45). The
  35-day `preferredGraceDays` matches the stale warning: if the preferred
  feed stops, the warning fires and the other format takes over at its next
  delivery.
  No guard compares row counts between deliveries, so switching formats
  (56k → 12k rows) trips nothing.

### Alert: `DistantSignalCorpusFormatDrift`

`sum by (namespace) (increase(distant_signal_schedule_feed_corpus_failures_total{stage="drift"}[corpusFailingWindow])) > 0`,
`for: 0m`, `schedulePipeline.severity`. This is the same counter and style as
`DistantSignalCorpusLoadFailing`, which now excludes `stage="drift"`.

- It fires on the first evaluation after the scan that sees the file, which
  is within one `pollIntervalSecs` (120 s) plus a scrape. It does not wait
  for the file to be stable.
- It clears `corpusFailingWindow` (30m) after the file is removed.
- It has a promtool test in `scripts/alert-rules-tests/corpus.yaml` and a
  runbook in `docs/alerts.md`.

### Chart

```yaml
scheduleFeed:
  corpus:
    enabled: false            # unchanged master switch
    jsonEnabled: true         # CORPUS_JSON_ENABLED
    filePattern: CORPUSExtract.json.gz
    minRows: 10000
    xml:
      enabled: true           # CORPUS_XML_ENABLED
      minRows: 10000          # CORPUS_XML_MIN_ROWS
    formatPrecedence: json    # CORPUS_FORMAT_PRECEDENCE (render fails on anything else)
    preferredGraceDays: 35    # CORPUS_PREFERRED_GRACE_DAYS
    compareFormats: true      # CORPUS_COMPARE_FORMATS
```

## Operator and deploy notes

- **No config change is needed** to start accepting the XML format. Both
  formats are on by default under the existing `scheduleFeed.corpus.enabled`.
- **The SFTP subscription.** To receive the XML format, subscribe the RDM
  "NWR CORPUS" product's new API version and point it at the same SFTP
  account and folder as the JSON extract (`dtd-push`, `incoming`). Keep the
  JSON subscription during the migration. No SFTP, Service or NetworkPolicy
  change is needed.
- **Migrating.** Watch the `compared the CORPUS JSON extract with the NLC
  XML snapshot` log line, or `schedule_feed_corpus_format_diff`, over a few
  deliveries. To switch, set `formatPrecedence: xml`. Then drop the JSON
  subscription, and later set `jsonEnabled: false`. The `json` default
  already falls back to XML on its own 35 days after the last JSON load.
- **The bucket source** (`scheduleFeed.bucket.expectedKeys`; that reader is
  not on `main` yet) would need `NLC*.xml.gz` added to receive the XML
  through GCS.
