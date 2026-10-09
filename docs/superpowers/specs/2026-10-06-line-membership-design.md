# Design: per-line train membership ("scope")

Status: implemented 2026-10-06 (schedule-reference, schedule-query, api,
full-coverage-consumer, line catalogue). Line pages do not use it yet.

## 1. Problem

A line's schedule population (`schedule_line_population`, published by
`schedule-reference`'s `publish_schedule_line_population`) is every
schedule touching any of the line's stations. Line pages
(`GET /public/lines/{id}/trains`) list all of them. At a hub that is
mostly other routes' trains: on 2026-10-06 the South West Main Line's
population held 2,890 schedules, of which about 320 are its own. Measured
precision of "touches a station" on 12 hand-labelled lines: 9%.

Full coverage (windowed stats, `2026-09-27-full-coverage-windowed-stats-design.md`
§4.1) narrows that to "the line's operator, calls at two of its stations",
which still counts every SWR train calling at Waterloo and Woking as a
South West Main Line train (998 a day).

## 2. Decision: scope per (line, train)

Each population entry carries a `scope`:

| scope | meaning | on the line page |
|-------|---------|------------------|
| `line` | one of the line's own trains | listed |
| `shared` | runs a stretch of the line, but is another route's or another operator's train (XC along Basingstoke–Bournemouth, TPE on Leeds–York, GWR on the Elizabeth line's western section) | listed (user decision) |
| `touch` | only touches the line: a hub call, a crossing | not listed; the UI links to the station page |

`touch` trains stay in the population: pin and schedule matching, movement
correlation and Delay Repay need them.

### 2.1 Rule "R75"

Per (line *L*, train *T*):

1. **Not a bus or ship** (CIF train status `B`, `5`, `S`, `4`).
2. **A verified run.** Consecutive *L* stations in *T*'s TIPLOC path, each
   pair joined by on-route TIPLOCs, covering ≥ 2 stations.
   - *L*'s route TIPLOCs are **learned** each day from *L*'s own operator's
     trains: every TIPLOC between two consecutive calls at *L* stations at
     most 3 catalogue positions apart (between catalogue-adjacent stations
     up to 6 stations the catalogue does not list may lie between, e.g.
     skipped minor stops; between stations further apart none may). When
     none of *L*'s operators runs there that day, every non-bus train of
     the population seeds it.
   - A stretch of *T* is accepted when at most one of its intermediate
     TIPLOCs was never learned (`UNKNOWN_SLACK`).
   - This copes with CIF recording passes only at timing points, branches,
     loops and lines whose catalogue lists only termini.
   - The longest chain of accepted stretches (most distinct stations) is
     the run.
3. **Operator** in *L*'s `operators`. If no train of *L*'s operators has a
   run on *L* that day, any operator counts (the Island Line fallback: the
   catalogue said `SW`, the trains run as `IL`).
4. **One of:**
   - the whole journey is on *L*'s route (every TIPLOC learned or an *L*
     station);
   - the run spans ≥ 75% of *L*'s catalogue stations (user decision);
   - *L* is *T*'s best fit among its operator's lines: most run stations,
     tie broken by the larger share of the line; ties keep all;
   - *T*'s best-fit line is in *L*'s new optional `trunk_for`.

Scope `line` passes all four; `shared` has a run but fails 1, 3 or 4;
`touch` has no run (or touches no station at all).

**Direction:** `down` when the run goes from an earlier catalogue station
to a later one, `up` the reverse, `loop` when the run's chain starts and
ends at the same station (the Cathcart Circle).

**`line_due`:** the train's first booked public call (public departure,
else public arrival) at one of *L*'s stations, Europe/London local time,
with its day offset. Independent of scope.

### 2.2 What counts as a station TIPLOC ("touch" logic)

A TIPLOC counts as station *S* when the CIF crosswalk (`tiploc_crs` ∪
`stanox_crs`, `tiploc_crs` winning) maps it to *S* **and some train of the
day calls at it**. A TIPLOC that is only ever passed is a junction or
timing point even when the crosswalk files it under a station's CRS:
`ACTONW` (Acton West) → `EAL` and `NWTLEJ` (Newton East Jn) → `NTN` are
dropped this way. The crosswalk itself, and every other consumer of it
(interchange, station boards, `tiploc_crs` API reads), is unchanged; only
the population's touch test and membership use the narrower set.

A line's `crs_aliases` add the timetable's sub-CRS for some platforms (see
§3).

## 3. Catalogue (Phase 0)

- **`crs_aliases`** (new, optional, per line): CIF CRS → catalogue
  station. The Elizabeth line's central platforms have their own TIPLOC and
  CRS: `PDX`/`FDX`/`LSX`/`WHX`/`ABX` for `PAD`/`ZFD`/`LST`/`ZLW`/`ABW`;
  Thameslink's St Pancras platforms are `SPL` for `STP`. The catalogue keeps
  the board CRS because station pages, LDBWS sampling (`sample_stations`),
  the `stop_board` overlay (which already maps `PADTLL` to `PAD`'s board
  through `station_samples.tiplocs`) and the incident matcher all use it;
  rewriting the catalogue to `PDX` would have broken every one of them.
  Set on `elizabeth-line`, `elizabeth-heathrow`, `elizabeth-shenfield`,
  `thameslink-core`, `thameslink-bedford`, `thameslink-cambridge`,
  `thameslink-rainham`.
- **`trunk_for`** (new, optional): `lner-ecml` → `lner-leeds`,
  `lner-lincoln`, `lner-hull`; `cross-country` → `xc-cardiff`,
  `xc-manchester`, `xc-south-coast`, `xc-stansted`; `thameslink-core` →
  `thameslink-bedford`, `thameslink-cambridge`, `thameslink-rainham`,
  `thameslink-southern`.
- `line-catalogue-validator` checks both: alias keys are real CRS not on
  the line, values are the line's stations; `trunk_for` ids exist and share
  an operator.
- **Island Line** `operators = ["IL"]` (was `SW`; CIF and TRUST use `IL`).
- **Basingstoke**: one narrow shared segment `swr-basingstoke-junction` in
  `swr-south-west-main`, `swr-west-of-england` and `xc-south-coast`, per
  `lines/SCHEMA.md`'s junction rule (it was three different exclusive
  segments).

## 4. Implementation

- `schedule_query::line_membership` (pure): `DayTrains` (every
  non-cancelled schedule of a date, TIPLOCs interned, call flags,
  operator, status), `classify(day, lines, crosswalk)` → per line the
  members with `Membership { scope, run_first_crs, run_last_crs,
  direction }` and the line's station TIPLOCs; `line_due(...)`.
- `schedule-reference`: per date, resolve every UID once into `DayTrains`,
  classify all lines together (best fit compares lines; a line learns its
  route from its own trains), then re-resolve each line's members and
  publish. This replaces resolving every UID once per line
  (`schedules_touching`), so the publish is cheaper than before: the full
  2026-10-06 day (25,127 schedules × 243 lines) classifies in about 0.5 s
  (release build). Extra memory: the interned day, a few MB. One `info!`
  per line and date logs the scope counts.
- `LinePopulationEntry` gains optional `scope`, `run_first_crs`,
  `run_last_crs`, `direction`, `line_due` — `#[serde(default,
  skip_serializing_if = "Option::is_none")]`, the `operator_atoc`
  version-skew pattern: an old reader ignores them, an old population reads
  `None`.

## 5. API (Phase 2)

`GET /public/lines/{id}/trains` and `/schedule` take an optional
`scope=line|shared|touch` (comma list) or `all`, filtered in SQL. The
default (no `scope`) is unchanged, for MCP and other external consumers
(user decision: additive). The line page will pass `scope=line,shared`.
With `scope`, the header `x-scope-applied` is `true`, or `false` when the
population predates the fields — then nothing is filtered (a header
because both bodies are bare arrays). `/trains` entries carry `scope`,
`direction`, `runFirstCrs`, `runLastCrs`, `lineDue {time, dayOffset}`,
each omitted when absent. See `docs/api-changelog.md`.

## 6. Full coverage (Phase 3)

`FULL_COVERAGE_LINE_MEMBERSHIP=legacy|scope|shadow` (chart
`fullCoverageConsumer.windowedStats.lineMembership`, chart default `scope`
since batch 54, as production runs it; the binary's default is `legacy`):
see the windowed-stats design's "Decisions (2026-10-06, line membership)".
Expected effect per day: SWML 998 → 319, Leeds–York 200 → 67, Elizabeth
678 → 336; all lines 62,930 → 28,319.

## 7. Measurement

Production extract for rail day 2026-10-06 (read-only SELECTs: every
population schedule's calling points, operator, status; the crosswalks),
the worktree catalogue, and the real Rust `classify`, against the
hand-written route truth of the analysis (`core` per line, from operator
and whole CRS path):

| line | population | own trains | kept as `line` | precision | recall |
|------|-----------:|-----------:|---------------:|----------:|-------:|
| swr-south-west-main | 2,890 | 318 | 319 | 99.4% | 99.7% |
| gwr-windsor-branch | 671 | 110 | 110 | 100% | 100% |
| swr-lymington-branch | 236 | 66 | 66 | 100% | 100% |
| northern-leeds-york | 1,412 | 66 | 67 | 98.5% | 100% |
| northern-harrogate-line | 1,412 | 80 | 80 | 100% | 100% |
| northern-airedale | 1,278 | 217 | 217 | 100% | 100% |
| thameslink-core | 2,372 | 566 | 571 | 99.1% | 100% |
| elizabeth-line | 4,070 | 335 | 336 | 99.7% | 100% |
| scotrail-cathcart-circle | 1,085 | 197 | 197 | 100% | 100% |
| swr-chertsey-loop | 2,275 | 74 | 78 | 93.6% | 98.6% |
| cross-country | 4,955 | 157 | 154 | 100% | 98.1% |
| lner-ecml | 3,281 | 200 | 200 | 100% | 100% |
| **all 12** | | 2,386 | 2,395 | **99.4%** | **99.8%** |

Identical, train for train, to the Python prototype of the analysis.
The disagreements: Thameslink's five "extra" trains are Sutton loop–Bedford
core trains the truth missed (it did not map `SPL` to `STP`, so they had
two core stations, not three) -- real precision there is 100%;
CrossCountry's three misses are Cardiff–Birmingham trains whose best fit is
`xc-cardiff` (the truth counted any XC train with two of the line's
stations); Leeds–York's extra is a Selby–York Northern train via Church Fenton and Ulleskelf. Over
all 243 lines the day's 302,173 (line, train) pairs split 28,324 `line`,
80,311 `shared`, 193,538 `touch`.

**Test fixture.** `crates/schedule-query/tests/fixtures/line-membership-2026-10-06.txt`
is the same extract reduced to the trains the tests name plus up to three
of each of their lines' own longest-running trains (route-learning seeds),
the crosswalk rows they use, and a snapshot of the catalogue lines they
touch (so later catalogue edits do not move the expectations). Every named
expectation was checked against the full day first and holds on the
reduced fixture. Regenerate it (with a new extract) when the rule or the
snapshot must change.

## 8. Rollout and deploy notes

1. Deploy `schedule-reference`, `api`, `full-coverage-consumer` together or
   in any order: every reader treats a population without `scope` as
   before.
2. **The new fields appear at the next CIF delivery `schedule-reference`
   processes** (nightly), for that day and the next. A restart does NOT
   republish an already-processed delivery (`last_processed_delivery` is
   seeded from api). Until then `?scope=` answers `x-scope-applied: false`
   unfiltered. Dates published before the deploy never gain the fields.
3. That first publish changes every population row (new fields), so `api`
   rewrites each row once and `full-coverage-consumer` re-downloads each
   population once (its `ETag` changes).
4. Set `fullCoverageConsumer.windowedStats.lineMembership: shadow`, watch
   the `full_coverage_consumer_line_membership_*` gauges and the shadow
   logs for a few days, then `scope`.
5. Frontend: pass `scope=line,shared` on the line page and link `touch`
   trains to station pages (not done here).

## 9. Risks and open points

- **Learning needs the line's own trains.** A line whose operator runs
  nothing along it on a day falls back to every train (Island Line case);
  a line run only by trains that the catalogue's station order mislabels
  learns a wrong route. The validator cannot check this; the shadow
  metrics will show it.
- **Best fit is relative to the catalogue.** Adding a line (the concurrent
  Southern West London line, Marshlink and TPE work) can move a train's
  best fit and so its `line` scope on another line of the same operator.
  Intended, but worth a look in the shadow comparison after catalogue
  changes.
- **Buses** are never `line`, but a rail-replacement bus along the line is
  `shared` and so appears with `scope=line,shared`.
- `swr-chertsey-loop` is the weakest line (93.6% precision): its four
  extra trains are Waterloo–Hounslow services via Brentford, whose run
  along the catalogue's shared Waterloo–Barnes section makes it their best
  fit among SWR's lines; one Weybridge–Chertsey working is missed.
- The fixed 75% span threshold and the one-unknown-TIPLOC slack were
  chosen on one day's data.
- `line_due` uses the public time; a public time that crosses midnight
  against the working time takes the working time's day offset.
