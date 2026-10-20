# TfL stop lookup for untracked legs (2026-10-20)

Status: design, implemented on the branch that adds this file. Work log
DS-79 (a subtask of DS-66). Buses are DS-77, a later card.

Untracked legs (`2026-10-20-untracked-legs-and-tracked-train-states-design.md`)
take free-text places. This adds a lookup of London Underground, DLR and
tram stops, so a leg can name a real stop and carry its id, and so the
trip planner's Tube transfers between National Rail stations can name the
Tube stations at each end.

Decisions made by the operator before this design:

- The source is TfL's Unified API (`StopPoint`), with the subscription key
  poller-tfl already has. NaPTAN is a fallback only, and is not needed.
- The first cut covers Tube, DLR and tram. Overground and the Elizabeth
  line stay on the National Rail station search (`GET /stations`).
- Legs store stop ids.
- Planner transfers are linked to stops.
- Buses come later.

## 1. Producer: poller-tfl's stops job

poller-tfl gains a second loop, off by default (`TFL_STOPS_ENABLED`, chart
`pollers.tfl.stops.enabled`), that runs once a day
(`TFL_STOPS_INTERVAL_SECS`, default 86400):

1. `GET /StopPoint/Mode/tube,dlr,tram?page=N` for N = 1, 2, ... until a
   page has fewer than 1000 stop points (at most 20 pages). The response
   is a `StopPointsResponse` (`stopPoints`, `pageSize`, `total`, `page`);
   a bare array is accepted too.
2. Keeps the station-level stop points: a `naptanId` that starts `940G`
   (a NaPTAN metro or tram stop area), with at least one of the modes
   `tube`, `dlr`, `tram`. Children are walked too, so a nested response
   gives the same result. Platforms, entrances and access areas (`9400…`)
   are dropped.
3. For every hub those stations name (`hubNaptanCode`, `HUB…`),
   `GET /StopPoint/{ids}` in batches of 20 reads the hub's children. A
   child that is a National Rail station (`910G<TIPLOC>`) gives the hub its
   rail TIPLOCs.
4. Publishes one `tfl-stops/1` entry on `ds:ingest:tfl` (the poller's own
   stream, so no new ACL), one part, whose body is the array of
   `common::tfl_stops::TflStop`:
   - `id`: the NaPTAN id (`940GZZLUEUS`);
   - `name`: cleaned (`Euston Underground Station` -> `Euston`; suffixes
     ` Underground Station`, ` DLR Station`, ` Tram Stop`, ` Station`);
   - `modes`: the in-scope modes; `lines`: `{id, name}` of its lines in
     those modes (`lineModeGroups` decides which line belongs to which
     mode);
   - `hubCode`, `lat`, `lon`;
   - `crs`: for a hub with a rail station, the hub code's three letters
     when it is `HUB[A-Z]{3}` (`HUBEUS` -> `EUS`; TfL's hub codes are CRS
     codes), and `railTiplocs`, the hub's rail TIPLOCs (`EUSTON`), which
     the writer resolves through `tiploc_crs` (so `HUBKGX` also links
     `STP` through `STPX`).

Requests go one at a time with a 250 ms gap, about 10 calls a day, far
under TfL's 500 a minute; the existing retry and backoff
(`fetch_json`) applies. A 4xx other than 429 fails the run.

**Feed guard.** A snapshot is refused (not published; the previous one
stays) when it has no stops or when its stop count fell by more than
`TFL_STOPS_MAX_DROP_FRACTION` (default 0.2) from the last published
`tfl-stops/1` on the stream (read back with
`ingest_stream::snapshot::newest_of_schema`, so it survives a restart).

**Metrics.** `distant_signal_tfl_fetch_total{what="stops"|"stop-hubs"}`
(the existing fetch counter), `distant_signal_tfl_stops_published` (gauge:
stops in the last snapshot), `distant_signal_tfl_stops_refused_total{reason}`
(`empty`, `sharp_drop`) and `distant_signal_tfl_stops_last_success_timestamp_seconds`.

The line-status loop's startup cursor becomes its own schema's newest
entry, so a stops entry never delays a line-status poll.

**Budget.** `ds:ingest:tfl` keeps MAXLEN 288 (a day of line status). Its
declared worst-case entry grows from 2 KiB to 24 KiB for the stops entry
(about 400 stops, gzip): about 19 MB worst case for the stream and its
dead letters, under the 75% alert line.

## 2. Writer

`ingest-writer`'s `tfl` stream carries `tfl-stops` as well as
`tfl-line-status`; the `tfl-stops/1` handler is always registered, so it
needs no new switch. Deploy the writer before turning the poller's switch
on: an older writer dead-letters `tfl-stops` as not carried by the stream.

The handler (in the writer's transaction, with its `ingest_dedup` row)
calls `ds_store::tfl_stops::replace`:

- an empty snapshot writes nothing (the producer never sends one);
- an older snapshot than the stored one is skipped (`fetched_at`);
- upserts every stop into `tfl_stops`, deletes stops absent from the
  snapshot, and rebuilds `tfl_stop_crs` (hub CRS plus `tiploc_crs`
  matches of `railTiplocs`);
- records freshness `tfl-stops`.

Migration `20261020160000_tfl_stops.sql` (additive):

- `tfl_stops (stop_id PK, name, search_name, modes TEXT[], lines JSONB,
  hub_code, latitude, longitude, fetched_at)`; `search_name` is the
  name lower-cased with punctuation dropped and `&` as `and`
  (`common::tfl_stops::search_key`), so "kings cross" finds "King's Cross
  St. Pancras";
- `tfl_stop_crs (stop_id -> tfl_stops ON DELETE CASCADE, crs, source)`,
  primary key `(stop_id, crs)`, indexed on `crs`.

Grants (`db-grants.yaml`, class `reference`): `tfl_stops` api S, writer
SIUD; `tfl_stop_crs` api S, writer SID. The writer's `ingest_freshness`
row policy gains `tfl-stops`.

No `pg_trgm`: it is not installed (no migration creates it), and the
table has about 400 rows. The search is the station search's tiered
`ILIKE` (`data::reference::search_stations`) over `search_name`, with a
word-prefix tier, and no index.

## 3. API

- `GET /public/tfl/stops?q=&modes=` (unauthenticated, like `/stations`):
  up to 20 `{id, name, modes, lines: [{id, name}]}`, ranked: exact name,
  name prefix, a word of the name starting with the query, then any
  substring; alphabetical within each. `modes` is a comma list of `tube`,
  `dlr`, `tram` (default all three); an unknown mode is a 400. A blank `q`
  is `[]`.
- `GET /public/tfl/stops/by-crs?crs=EUS[&modes=]`: the TfL stops for a
  National Rail station (section 5's mapping), `[]` when none.

## 4. Stop ids on legs

Migrations `20261020160100_journey_legs_stop_ids.sql` and `..._validate`:
nullable `origin_stop_id` and `destination_stop_id` on `journey_legs`,
and a CHECK `journey_legs_stop_ids_check`: both NULL on a train leg, and
each, when set, matches `^[0-9A-Z]{4,20}$`. Added NOT VALID and validated
by the next migration, as before. The existing untracked shape CHECK is
left alone (it says nothing about the new columns); no FK, since a stop
can leave TfL's data while a leg keeps its id.

- `POST /Journeys`, `POST /Journeys/{id}/legs` and `POST
  /Journeys/itinerary` untracked legs take optional `originStopId` and
  `destinationStopId`. Each must be a stop in `tfl_stops` (400 otherwise).
  A blank `originName`/`destinationName` is filled with the stop's name.
- Legs gain `originStopId`/`destinationStopId` on `GET /Journeys/{id}`,
  the share-link read and the delegated journey read; `null` on a train
  leg or a free-text place. The account export includes both columns
  (`to_jsonb` of the row).
- `transitMode` gains `dlr`.

## 5. Planner transfers

`/Trips/plan` transfer legs (`kind: "transfer"`) whose `mode` is `TUBE` or
`METRO` gain optional `originStop` and `destinationStop`
(`{id, name, modes, lines}`), absent when unknown. A CRS maps to a stop by,
in order:

1. a small override list in code (`data::tfl_stops::CRS_OVERRIDES`);
2. the crosswalk (`tfl_stop_crs`), preferring a Tube stop;
3. the nearest Tube stop within 300 m of the station's `stations`
   latitude and longitude.

The same mapping backs `GET /public/tfl/stops/by-crs`. The planner's save
flow (DS-76, another branch) turns a transfer into an untracked leg; with
these fields it can send `originName`/`originStopId` from `originStop`.

## 6. Frontend

`UntrackedLegFields`: for Tube, DLR and tram, From and To are an
`Autocomplete` over `GET /tfl/stops` (`useSuggestions`,
`suggestionAutocompleteProps` with the name, not the code, written into
the field). Picking a stop fills the place name and the stop id; editing
the text afterwards drops the id. Free text stays allowed. Other modes
keep the plain text inputs.

## 7. Attribution

TfL's terms ask for "Powered by TfL Open Data" (already in the footer and
on `/attribution`) and, for data derived from Ordnance Survey and Geomni
(StopPoint coordinates are), "Contains OS data © Crown copyright and
database rights 2016" and "Geomni UK Map data © and database rights
[2019]". Both join the TfL entry on `/attribution` and the footer's TfL
line. No TfL roundel, logo or name styling is used.

## 8. Operator notes

Nothing changes until the switch is on. To turn it on:

1. Deploy the images (the migrations run with the api's migrate job).
2. Re-run the grants job (`postgres-grants.sql`) so the writer can write
   `tfl_stops`/`tfl_stop_crs` and record `tfl-stops` freshness.
3. Set `pollers.tfl.stops.enabled: true`. No Redis ACL change: the stops
   go on `ds:ingest:tfl`, which poller-tfl may already `XADD` to, and the
   writer already applies (`ingestWriter.streams.tfl: apply`).

The first run happens at start; afterwards daily. Check
`distant_signal_tfl_stops_published` and `ingest_freshness` source
`tfl-stops`.
