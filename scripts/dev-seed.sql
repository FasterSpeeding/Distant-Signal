-- =========================================================================
-- FABRICATED DEMO DATA — local dev-seeding convenience, not a fixture any
-- code path depends on.
-- =========================================================================
--
-- Originally written 2026-09-22 to make a local instance's pages render
-- non-empty content for a screenshot-driven UX review. Kept in the repo
-- afterward as a documented convenience for anyone who wants a running
-- local instance with realistic-looking data without needing live
-- RDM/LDBWS feed credentials. NONE of this is real railway data — every
-- "live" value below (train positions, delays, platforms, incidents, line
-- status) is invented to exercise the UI, not observed. Do not treat any of
-- it as evidence about real services, and do not run it against anything
-- but a disposable local/dev database.
--
-- Deliberately idempotent (ON CONFLICT everywhere) so it can be re-run.
--
-- Apply with (from the repo root):
--   PGPASSWORD=postgres psql -h localhost -U postgres -d postgres \
--     -v ON_ERROR_STOP=1 -f scripts/dev-seed.sql
--
-- Design notes that determine whether rows are actually VISIBLE in the UI:
--   * `line_status.line_id` MUST match an id from a real `lines/*.toml`
--     file, otherwise /lines, /lines/[id] and the operator rollup all skip
--     it (the catalogue is loaded from LINES_DIR at API startup, not the DB).
--   * An operator only appears on /operators if BOTH a `tocs` row exists
--     AND at least one catalogue `line_status` row lists its ATOC code in
--     `operators`. A `tocs` row alone renders nothing.
--   * `/public/trains/search` defaults to TODAY in Europe/London and to
--     "from the current clock time", and 404s outright if NOTHING at all is
--     published for that service_date. Schedule rows are therefore seeded
--     for CURRENT_DATE across a full spread of times.
--   * The /incidents UI defaults its date filter to the last 30 days, so
--     `first_seen_at` is seeded as recent.
-- =========================================================================

BEGIN;

-- -------------------------------------------------------------------------
-- Operators (tocs). Real ATOC codes and names -- this is public reference
-- data, the one genuinely non-fabricated part of this file.
-- -------------------------------------------------------------------------
INSERT INTO tocs (atoc_code, name, legal_name, atoc_member, station_operator, fetched_at) VALUES
    ('GR', 'London North Eastern Railway', 'London North Eastern Railway Limited', TRUE, TRUE,  NOW()),
    ('GW', 'Great Western Railway',        'First Greater Western Limited',        TRUE, TRUE,  NOW()),
    ('SW', 'South Western Railway',        'First MTR South Western Trains Limited', TRUE, TRUE, NOW()),
    ('SE', 'Southeastern',                 'SE Trains Limited',                    TRUE, TRUE,  NOW()),
    ('NT', 'Northern',                     'Northern Trains Limited',              TRUE, TRUE,  NOW()),
    ('SR', 'ScotRail',                     'ScotRail Trains Limited',              TRUE, TRUE,  NOW()),
    ('VT', 'Avanti West Coast',            'First Trenitalia West Coast Rail Limited', TRUE, TRUE, NOW()),
    ('XC', 'CrossCountry',                 'XC Trains Limited',                    TRUE, FALSE, NOW())
ON CONFLICT (atoc_code) DO UPDATE SET
    name = EXCLUDED.name, legal_name = EXCLUDED.legal_name, fetched_at = EXCLUDED.fetched_at;

-- -------------------------------------------------------------------------
-- Stations. Real CRS codes/names (public reference data); the accessibility
-- blob on KGX is a trimmed, invented sample just so that section renders.
-- -------------------------------------------------------------------------
INSERT INTO stations (crs, name, latitude, longitude) VALUES
    ('KGX', 'London Kings Cross',   51.5308, -0.1238),
    ('PAD', 'London Paddington',    51.5154, -0.1755),
    ('WAT', 'London Waterloo',      51.5033, -0.1124),
    ('EUS', 'London Euston',        51.5282, -0.1337),
    ('CHX', 'London Charing Cross', 51.5085, -0.1247),
    ('YRK', 'York',                 53.9580, -1.0933),
    ('NCL', 'Newcastle',            54.9686, -1.6174),
    ('EDB', 'Edinburgh',            55.9521, -3.1892),
    ('DON', 'Doncaster',            53.5219, -1.1400),
    ('PBO', 'Peterborough',         52.5744, -0.2500),
    ('RDG', 'Reading',              51.4589, -0.9721),
    ('BRI', 'Bristol Temple Meads', 51.4492, -2.5813),
    ('SWI', 'Swindon',              51.5654, -1.7855),
    ('WOK', 'Woking',               51.3181, -0.5567),
    ('SOU', 'Southampton Central',  50.9073, -1.4136),
    ('BSK', 'Basingstoke',          51.2681, -1.0873),
    ('LDS', 'Leeds',                53.7955, -1.5491),
    ('SKI', 'Skipton',              53.9592, -2.0271),
    ('SHP', 'Shipley',              53.8331, -1.7736),
    ('BDQ', 'Bradford Forster Square', 53.7961, -1.7519),
    ('GLC', 'Glasgow Central',      55.8587, -4.2576),
    ('MAN', 'Manchester Piccadilly', 53.4774, -2.2309),
    ('BHM', 'Birmingham New Street', 52.4778, -1.8980),
    ('ASH', 'Ashford International', 51.1434, 0.8748),
    ('CBW', 'Canterbury West',      51.2848, 1.0724)
ON CONFLICT (crs) DO UPDATE SET
    name = EXCLUDED.name, latitude = EXCLUDED.latitude, longitude = EXCLUDED.longitude;

-- Only the 12 allowlisted top-level keys survive `filter_accessibility_fields`
-- (crates/api/src/data/reference.rs) -- anything else is stripped, so the
-- shape below has to use those exact camelCase names.
UPDATE stations
SET accessibility = '{
  "stationAccessibility": {
    "stepFreeAccess": "Step-free access to all platforms via lifts.",
    "wheelchairAvailable": true,
    "inductionLoop": true
  },
  "staffAssistance": {
    "staffing": "Staffed from first to last train.",
    "assistanceAvailable": true
  },
  "lifts": {"count": 6, "note": "Lifts serve all platforms."},
  "toiletsAndChanging": {"accessibleToilets": true, "nationalKeyToilets": true, "babyChanging": true},
  "carParks": {"spaces": 0, "accessibleSpaces": 0, "note": "No station car park."},
  "dropOffPickUp": {"note": "Set-down point on Euston Road, east side."},
  "platformFacilities": {"seating": true, "shelters": true, "customerHelpPoints": true},
  "helpAndSupport": {"helpPoints": true, "note": "Passenger Assist bookable up to 2 hours before travel."},
  "loungesAndWaiting": {"firstClassLounge": true, "waitingRoom": true},
  "transportLinks": {"undergroundInterchange": true, "busInterchange": true},
  "cycling": {"cycleParkingSpaces": 120, "sheltered": true},
  "stationFacilities": {"shops": true, "atm": true, "wifi": true}
}'::jsonb
WHERE crs = 'KGX';

-- -------------------------------------------------------------------------
-- Line status. THE table behind /, /lines, /lines/[id], /status and the
-- /operators rollup. Deliberately spans "various states" for the audit:
-- a clean good-service line, minor delays, severe delays, part suspension,
-- and a planned closure.
--
-- `severity` numbers are `common::Severity` discriminants:
--   2 = Suspended, 3 = PartSuspended, 4 = PlannedClosure,
--   6 = SevereDelays, 9 = MinorDelays, 10 = GoodService.
-- -------------------------------------------------------------------------
INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source, computed_at) VALUES

-- SEVERE DELAYS, with sample stats and a linked incident -- the "worst case"
-- card for the audit.
('lner-ecml', 'LNER East Coast Main Line', 'national-rail', '{GR}', '[{
    "severity": 6,
    "reason": "Severe delays between London Kings Cross and Peterborough after a signalling failure. Services may be delayed by up to 40 minutes, revised or cancelled.",
    "validity": {"from_date": "2026-09-22T06:00:00Z", "to_date": null, "is_now": true},
    "data_quality": "knowledgebase",
    "sample_stats": {"total": 48, "delayed": 21, "cancelled": 4, "skipped": 2, "avg_delay_minutes": 18.4},
    "sample_availability": {"state": "available", "total": 48, "delayed": 21, "cancelled": 4, "skipped": 2, "avg_delay_minutes": 18.4},
    "full_coverage_availability": {"state": "not-enabled"},
    "disruption": {"category": "RealTime", "description": "Severe delays between London Kings Cross and Peterborough after a signalling failure.", "affected_stops": ["KGX", "PBO", "DON", "YRK"], "affected_routes": [], "source": "knowledgebase-incident-PREVIEW-INCIDENT-1"}
}]'::jsonb, 'aggregator', NOW()),

-- MINOR DELAYS.
('gwr-main-line', 'GWR Great Western Main Line', 'national-rail', '{GW}', '[{
    "severity": 9,
    "reason": "Minor delays between Reading and Swindon following an earlier trespass incident. Services are returning to normal.",
    "validity": {"from_date": "2026-09-22T07:30:00Z", "to_date": null, "is_now": true},
    "data_quality": "knowledgebase",
    "sample_stats": {"total": 36, "delayed": 7, "cancelled": 0, "skipped": 1, "avg_delay_minutes": 6.2},
    "sample_availability": {"state": "available", "total": 36, "delayed": 7, "cancelled": 0, "skipped": 1, "avg_delay_minutes": 6.2},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW()),

-- GOOD SERVICE -- the clean baseline the audit explicitly wants alongside
-- the disrupted ones.
('swr-south-west-main', 'SWR South West Main Line', 'national-rail', '{SW}', '[{
    "severity": 10,
    "reason": "Good service",
    "validity": {"from_date": "2026-09-22T00:00:00Z", "to_date": null, "is_now": true},
    "data_quality": "ldbws-inferred",
    "sample_stats": {"total": 52, "delayed": 2, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.1},
    "sample_availability": {"state": "available", "total": 52, "delayed": 2, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.1},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW()),

-- GOOD SERVICE, but with no live sampling behind it -- exercises the
-- "no-coverage" data-quality badge, a visually distinct third state.
('southeastern-main-line', 'Southeastern Main Line', 'national-rail', '{SE}', '[{
    "severity": 10,
    "reason": "Good service",
    "validity": {"from_date": "2026-09-22T00:00:00Z", "to_date": null, "is_now": true},
    "data_quality": "knowledgebase",
    "sample_availability": {"state": "no-coverage"},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW()),

-- PART SUSPENDED.
('northern-airedale', 'Northern Airedale Line', 'national-rail', '{NT}', '[{
    "severity": 3,
    "reason": "No service between Shipley and Skipton due to a landslip. Replacement road transport has been requested.",
    "validity": {"from_date": "2026-09-22T05:00:00Z", "to_date": null, "is_now": true},
    "data_quality": "knowledgebase",
    "sample_stats": {"total": 22, "delayed": 5, "cancelled": 9, "skipped": 3, "avg_delay_minutes": 14.0},
    "sample_availability": {"state": "available", "total": 22, "delayed": 5, "cancelled": 9, "skipped": 3, "avg_delay_minutes": 14.0},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW()),

-- PLANNED CLOSURE (a future-dated engineering works entry).
('scotrail-fife-circle', 'ScotRail Fife Circle', 'national-rail', '{SR}', '[{
    "severity": 4,
    "reason": "Planned engineering work will close the line between Edinburgh and Kirkcaldy next weekend. Replacement buses will operate.",
    "validity": {"from_date": "2026-09-26T22:00:00Z", "to_date": "2026-09-29T05:00:00Z", "is_now": false},
    "data_quality": "knowledgebase",
    "sample_availability": {"state": "no-coverage"},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW()),

-- Two more good-service lines so the operator rollup has more than one line
-- per operator to average over.
('wcml-manchester', 'West Coast Main Line (Manchester)', 'national-rail', '{VT}', '[{
    "severity": 10,
    "reason": "Good service",
    "validity": {"from_date": "2026-09-22T00:00:00Z", "to_date": null, "is_now": true},
    "data_quality": "ldbws-inferred",
    "sample_stats": {"total": 30, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 2.4},
    "sample_availability": {"state": "available", "total": 30, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 2.4},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW()),

('cross-country', 'CrossCountry', 'national-rail', '{XC}', '[{
    "severity": 9,
    "reason": "Minor delays across the CrossCountry network following earlier congestion around Birmingham New Street.",
    "validity": {"from_date": "2026-09-22T08:00:00Z", "to_date": null, "is_now": true},
    "data_quality": "ldbws-inferred",
    "sample_stats": {"total": 41, "delayed": 11, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 8.7},
    "sample_availability": {"state": "available", "total": 41, "delayed": 11, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 8.7},
    "full_coverage_availability": {"state": "not-enabled"}
}]'::jsonb, 'aggregator', NOW())

ON CONFLICT (line_id) DO UPDATE SET
    name = EXCLUDED.name, mode_name = EXCLUDED.mode_name, operators = EXCLUDED.operators,
    statuses = EXCLUDED.statuses, source = EXCLUDED.source, computed_at = EXCLUDED.computed_at;

-- A short history trail so the per-line history/trends view isn't blank.
INSERT INTO line_status_history (line_id, statuses, computed_at)
SELECT ls.line_id, ls.statuses, NOW() - (g.n || ' hours')::interval
FROM line_status ls
CROSS JOIN generate_series(1, 12) AS g(n)
WHERE ls.line_id IN ('lner-ecml', 'gwr-main-line', 'swr-south-west-main', 'northern-airedale');

-- Daily + half-hourly rollups behind the Trends tabs and the operator/network
-- historical views (operator-overview Phase 4).
INSERT INTO line_status_daily_stats
    (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum)
SELECT ls.line_id,
       (CURRENT_DATE - g.n),
       24,
       200 + (g.n * 7),
       20 + (g.n * 3),
       (g.n % 4),
       (g.n % 3),
       180 + (g.n * 4),
       140.0 + (g.n * 11)
FROM line_status ls
CROSS JOIN generate_series(0, 13) AS g(n)
ON CONFLICT (line_id, day) DO UPDATE SET
    total = EXCLUDED.total, delayed = EXCLUDED.delayed, cancelled = EXCLUDED.cancelled,
    skipped = EXCLUDED.skipped, running_count = EXCLUDED.running_count,
    delay_minutes_sum = EXCLUDED.delay_minutes_sum, sample_cycles = EXCLUDED.sample_cycles;

INSERT INTO line_status_half_hourly_stats
    (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum)
SELECT ls.line_id,
       date_trunc('hour', NOW()) - (g.n * interval '30 minutes'),
       10,
       40 + g.n,
       4 + (g.n % 6),
       (g.n % 3),
       (g.n % 2),
       36 + g.n,
       22.0 + g.n
FROM line_status ls
CROSS JOIN generate_series(0, 47) AS g(n)
ON CONFLICT (line_id, half_hour_start) DO UPDATE SET
    total = EXCLUDED.total, delayed = EXCLUDED.delayed, cancelled = EXCLUDED.cancelled,
    skipped = EXCLUDED.skipped, running_count = EXCLUDED.running_count,
    delay_minutes_sum = EXCLUDED.delay_minutes_sum, sample_cycles = EXCLUDED.sample_cycles;

-- -------------------------------------------------------------------------
-- Incidents. `first_seen_at` is recent on purpose: the /incidents UI defaults
-- its date filter to the last 30 days.
-- -------------------------------------------------------------------------
INSERT INTO incidents (
    incident_id, summary, description, operators, affected_stations,
    priority, validity_periods, is_planned, is_cleared, fetched_at, first_seen_at, affected_lines
) VALUES
('PREVIEW-INCIDENT-1',
 'Signalling failure between London Kings Cross and Peterborough',
 'A signalling failure between London Kings Cross and Peterborough is causing severe delays of up to 40 minutes. Some services are being revised or cancelled. Network Rail engineers are on site. Customers may use their tickets on alternative routes.',
 '{GR}', '{KGX,PBO,DON,YRK}', 2,
 '[{"from_date":"2026-09-22T06:00:00Z","to_date":null,"is_now":true}]'::jsonb,
 FALSE, FALSE, NOW(), NOW() - interval '11 hours', '{lner-ecml}'),

('PREVIEW-INCIDENT-2',
 'Landslip between Shipley and Skipton',
 'A landslip following heavy rainfall has closed the line between Shipley and Skipton. No trains are able to run between these stations. Replacement road transport has been requested.',
 '{NT}', '{SHP,SKI,BDQ}', 1,
 '[{"from_date":"2026-09-22T05:00:00Z","to_date":null,"is_now":true}]'::jsonb,
 FALSE, FALSE, NOW(), NOW() - interval '13 hours', '{northern-airedale}'),

('PREVIEW-INCIDENT-3',
 'Planned engineering work: Edinburgh to Kirkcaldy',
 'Planned engineering work will close the line between Edinburgh and Kirkcaldy from late Saturday evening until early Monday morning. Replacement buses will operate throughout.',
 '{SR}', '{EDB}', 4,
 '[{"from_date":"2026-09-26T22:00:00Z","to_date":"2026-09-29T05:00:00Z","is_now":false}]'::jsonb,
 TRUE, FALSE, NOW(), NOW() - interval '4 days', '{scotrail-fife-circle}'),

('PREVIEW-INCIDENT-4',
 'Earlier trespass incident near Reading (cleared)',
 'Services between Reading and Swindon were disrupted by an earlier trespass incident. The line has since reopened and services are returning to normal.',
 '{GW}', '{RDG,SWI}', 3,
 '[{"from_date":"2026-09-22T07:30:00Z","to_date":"2026-09-22T09:15:00Z","is_now":false}]'::jsonb,
 FALSE, TRUE, NOW(), NOW() - interval '2 days', '{gwr-main-line}')

ON CONFLICT (incident_id) DO UPDATE SET
    summary = EXCLUDED.summary, description = EXCLUDED.description,
    operators = EXCLUDED.operators, affected_stations = EXCLUDED.affected_stations,
    priority = EXCLUDED.priority, validity_periods = EXCLUDED.validity_periods,
    is_planned = EXCLUDED.is_planned, is_cleared = EXCLUDED.is_cleared,
    affected_lines = EXCLUDED.affected_lines, fetched_at = EXCLUDED.fetched_at;

-- -------------------------------------------------------------------------
-- Scheduled departures -- backs /trains search, the station timetable section
-- on /stations/[crs], and /lines/[id]'s train list.
--
-- One row per (train, calling point). `origin_crs` is the CALLING POINT the
-- row is indexed under; `true_origin_crs` is where the working actually
-- starts. Seeded for CURRENT_DATE across the whole day so a search at any
-- clock time finds something.
-- -------------------------------------------------------------------------
DELETE FROM schedule_destination_departures
WHERE service_date = CURRENT_DATE AND train_uid LIKE 'P9%';

-- ECML: KGX -> YRK -> NCL -> EDB, departing every hour on the hour.
INSERT INTO schedule_destination_departures
    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs)
SELECT CURRENT_DATE, cp.dest, (time '06:00' + (h.n * interval '1 hour') + cp.off)::time,
       'P9E' || lpad(h.n::text, 3, '0'), cp.cp, 'KGX'
FROM generate_series(0, 15) AS h(n)
CROSS JOIN (VALUES
    ('KGX', 'EDB', interval '0 minutes'),
    ('PBO', 'EDB', interval '50 minutes'),
    ('DON', 'EDB', interval '1 hour 35 minutes'),
    ('YRK', 'EDB', interval '2 hours'),
    ('NCL', 'EDB', interval '2 hours 55 minutes')
) AS cp(cp, dest, off);

-- GWR: PAD -> RDG -> SWI -> BRI, every half hour.
INSERT INTO schedule_destination_departures
    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs)
SELECT CURRENT_DATE, cp.dest, (time '06:15' + (h.n * interval '30 minutes') + cp.off)::time,
       'P9W' || lpad(h.n::text, 3, '0'), cp.cp, 'PAD'
FROM generate_series(0, 29) AS h(n)
CROSS JOIN (VALUES
    ('PAD', 'BRI', interval '0 minutes'),
    ('RDG', 'BRI', interval '26 minutes'),
    ('SWI', 'BRI', interval '1 hour 2 minutes')
) AS cp(cp, dest, off);

-- SWR: WAT -> WOK -> BSK -> SOU, every half hour.
INSERT INTO schedule_destination_departures
    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs)
SELECT CURRENT_DATE, cp.dest, (time '05:45' + (h.n * interval '30 minutes') + cp.off)::time,
       'P9S' || lpad(h.n::text, 3, '0'), cp.cp, 'WAT'
FROM generate_series(0, 31) AS h(n)
CROSS JOIN (VALUES
    ('WAT', 'SOU', interval '0 minutes'),
    ('WOK', 'SOU', interval '25 minutes'),
    ('BSK', 'SOU', interval '48 minutes')
) AS cp(cp, dest, off);

-- Southeastern: CHX -> ASH -> CBW, hourly.
INSERT INTO schedule_destination_departures
    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs)
SELECT CURRENT_DATE, cp.dest, (time '06:40' + (h.n * interval '1 hour') + cp.off)::time,
       'P9K' || lpad(h.n::text, 3, '0'), cp.cp, 'CHX'
FROM generate_series(0, 15) AS h(n)
CROSS JOIN (VALUES
    ('CHX', 'CBW', interval '0 minutes'),
    ('ASH', 'CBW', interval '38 minutes')
) AS cp(cp, dest, off);

-- Tells /lines/[id]'s train list which trains belong to which line today.
-- `population` is a JSON array of OBJECTS -- routes/lines.rs reads
-- `entry["uid"]` and `entry["calling_points"]` off each element, so a bare
-- array of UID strings renders a list of blank rows (uid: null).
INSERT INTO schedule_line_population (line_id, service_date, population)
SELECT m.line_id, CURRENT_DATE, to_jsonb(array_agg(t.entry))
FROM (VALUES
    ('lner-ecml',            'P9E%'),
    ('gwr-main-line',        'P9W%'),
    ('swr-south-west-main',  'P9S%'),
    ('southeastern-main-line','P9K%')
) AS m(line_id, pat)
CROSS JOIN LATERAL (
    SELECT jsonb_build_object(
               'uid', d.train_uid,
               'calling_points', d.points
           ) AS entry
    FROM (
        SELECT sdd.train_uid,
               jsonb_agg(
                   jsonb_build_object(
                       'tiploc', sdd.origin_crs,
                       'crs', sdd.origin_crs,
                       'kind', CASE WHEN sdd.origin_crs = sdd.true_origin_crs THEN 'Origin' ELSE 'Intermediate' END,
                       'booked_arrival', NULL,
                       'booked_departure', to_char(sdd.scheduled, 'HH24:MI:SS'),
                       'is_half_minute_arrival', false,
                       'is_half_minute_departure', false
                   ) ORDER BY sdd.scheduled
               ) AS points
        FROM schedule_destination_departures sdd
        WHERE sdd.service_date = CURRENT_DATE AND sdd.train_uid LIKE m.pat
        GROUP BY sdd.train_uid
    ) AS d
) AS t
GROUP BY m.line_id
ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population;

-- -------------------------------------------------------------------------
-- Live station departure boards (station_samples) -- the per-station
-- "live departures" panel and the sample-stats section. Mixed states:
-- on time, delayed, and cancelled, with platform numbers so the platform
-- plumbing merged from the schedule-row branch has something to render.
-- -------------------------------------------------------------------------
INSERT INTO station_samples (crs, polled_at, departures) VALUES
('KGX', NOW(), '[
  {"service_id":"preview-kgx-1","operator":"GR","destination_crs":"EDB","scheduled":"17:30","estimated":"17:52","is_cancelled":false,"delay_minutes":22,"delay_reason":"a signalling failure","cancel_reason":null,"headcode":"1S15","skipped_stations":[],"platform":"1","planned_platform":"1"},
  {"service_id":"preview-kgx-2","operator":"GR","destination_crs":"YRK","scheduled":"17:48","estimated":"On time","is_cancelled":false,"delay_minutes":0,"delay_reason":null,"cancel_reason":null,"headcode":"1Y22","skipped_stations":[],"platform":"5","planned_platform":"5"},
  {"service_id":"preview-kgx-3","operator":"GR","destination_crs":"NCL","scheduled":"18:00","estimated":"Cancelled","is_cancelled":true,"delay_minutes":0,"delay_reason":null,"cancel_reason":"a signalling failure between London Kings Cross and Peterborough","headcode":"1N30","skipped_stations":[],"platform":null,"planned_platform":"7"},
  {"service_id":"preview-kgx-4","operator":"GR","destination_crs":"EDB","scheduled":"18:30","estimated":"18:41","is_cancelled":false,"delay_minutes":11,"delay_reason":"an earlier signalling failure","cancel_reason":null,"headcode":"1S25","skipped_stations":["PBO"],"platform":"2","planned_platform":"1"}
]'::jsonb),
('PAD', NOW(), '[
  {"service_id":"preview-pad-1","operator":"GW","destination_crs":"BRI","scheduled":"17:33","estimated":"On time","is_cancelled":false,"delay_minutes":0,"delay_reason":null,"cancel_reason":null,"headcode":"1C40","skipped_stations":[],"platform":"9","planned_platform":"9"},
  {"service_id":"preview-pad-2","operator":"GW","destination_crs":"SWI","scheduled":"17:45","estimated":"17:52","is_cancelled":false,"delay_minutes":7,"delay_reason":"an earlier trespass incident","cancel_reason":null,"headcode":"1B41","skipped_stations":[],"platform":"11","planned_platform":"11"},
  {"service_id":"preview-pad-3","operator":"GW","destination_crs":"BRI","scheduled":"18:03","estimated":"On time","is_cancelled":false,"delay_minutes":0,"delay_reason":null,"cancel_reason":null,"headcode":"1C44","skipped_stations":[],"platform":"8","planned_platform":"8"}
]'::jsonb),
('WAT', NOW(), '[
  {"service_id":"preview-wat-1","operator":"SW","destination_crs":"SOU","scheduled":"17:35","estimated":"On time","is_cancelled":false,"delay_minutes":0,"delay_reason":null,"cancel_reason":null,"headcode":"1W35","skipped_stations":[],"platform":"11","planned_platform":"11"},
  {"service_id":"preview-wat-2","operator":"SW","destination_crs":"WOK","scheduled":"17:42","estimated":"On time","is_cancelled":false,"delay_minutes":0,"delay_reason":null,"cancel_reason":null,"headcode":"2W42","skipped_stations":[],"platform":"5","planned_platform":"5"},
  {"service_id":"preview-wat-3","operator":"SW","destination_crs":"BSK","scheduled":"18:05","estimated":"On time","is_cancelled":false,"delay_minutes":0,"delay_reason":null,"cancel_reason":null,"headcode":"2B05","skipped_stations":[],"platform":"9","planned_platform":"9"}
]'::jsonb),
('YRK', NOW(), '[
  {"service_id":"preview-yrk-1","operator":"GR","destination_crs":"EDB","scheduled":"17:58","estimated":"18:20","is_cancelled":false,"delay_minutes":22,"delay_reason":"a signalling failure earlier in the journey","cancel_reason":null,"headcode":"1S15","skipped_stations":[],"platform":"3","planned_platform":"3"},
  {"service_id":"preview-yrk-2","operator":"XC","destination_crs":"BHM","scheduled":"18:12","estimated":"18:20","is_cancelled":false,"delay_minutes":8,"delay_reason":"congestion","cancel_reason":null,"headcode":"1V12","skipped_stations":[],"platform":"9","planned_platform":"9"}
]'::jsonb)
ON CONFLICT (crs) DO UPDATE SET polled_at = EXCLUDED.polled_at, departures = EXCLUDED.departures;

-- -------------------------------------------------------------------------
-- Demo user + session, so the authenticated pages (journeys, tracked trains,
-- groups, pinning) can actually be screenshotted. Interactive SSO login does
-- not work in this preview (SSO_ISSUER_URL is a placeholder), so the session
-- row is minted directly.
--
-- `sessions.id` is the SHA-256 hex digest of the RAW cookie value, exactly as
-- `crates/api/src/auth.rs`'s `hash_session_token` computes it. The raw token
-- the browser must send is:
--
--   Cookie: distant_signal_session=preview-demo-session-token-for-demo-user
--
-- -------------------------------------------------------------------------
INSERT INTO users (id, email, name, username)
VALUES ('demo-user', 'demo@example.invalid', 'Demo Reviewer', 'demo')
ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email, name = EXCLUDED.name;

DELETE FROM sessions WHERE user_id = 'demo-user';
INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at)
VALUES ('416c463f5add5e7968833be0a9c1e2bc2e6a1716d907e4807401140a3b5f941f',
        'demo-user', NULL, NOW(), NOW() + interval '14 days');

-- Pins, so the homepage's "Your lines" / "Your operators" sections and the
-- operator-overview pinning UI are populated rather than empty.
INSERT INTO pinned_lines (user_id, line_id) VALUES
    ('demo-user', 'lner-ecml'),
    ('demo-user', 'swr-south-west-main'),
    ('demo-user', 'northern-airedale')
ON CONFLICT DO NOTHING;

INSERT INTO pinned_operators (user_id, operator_code) VALUES
    ('demo-user', 'GR'),
    ('demo-user', 'SW')
ON CONFLICT DO NOTHING;

INSERT INTO pinned_stations (user_id, crs) VALUES
    ('demo-user', 'KGX'),
    ('demo-user', 'WAT')
ON CONFLICT DO NOTHING;

-- -------------------------------------------------------------------------
-- Tracked trains + journeys. Gives /journeys/[id], /track/mine and the
-- journey-tracking UI (multi-leg chaining, status badges, skip display)
-- real content.
-- -------------------------------------------------------------------------
DELETE FROM journey_legs WHERE journey_id IN (SELECT id FROM journeys WHERE user_id = 'demo-user');
DELETE FROM journeys WHERE user_id = 'demo-user';
DELETE FROM train_subscriptions WHERE user_id = 'demo-user';
DELETE FROM train_current_state WHERE trains_id IN (SELECT id FROM trains WHERE train_uid LIKE 'P9%');
DELETE FROM trains WHERE train_uid LIKE 'P9%';

-- Two real `trains` rows with live state: one delayed, one on time.
WITH t AS (
    INSERT INTO trains (train_uid, service_date) VALUES
        ('P9E010', CURRENT_DATE),
        ('P9S010', CURRENT_DATE)
    RETURNING id, train_uid
)
INSERT INTO train_current_state
    (trains_id, status, last_reported_location, last_event_type, delay_minutes,
     next_calling_point, updated_at)
SELECT t.id,
       'en_route',
       CASE WHEN t.train_uid = 'P9E010' THEN 'Doncaster' ELSE 'Woking' END,
       'DEPARTURE',
       CASE WHEN t.train_uid = 'P9E010' THEN 22 ELSE 0 END,
       CASE WHEN t.train_uid = 'P9E010' THEN 'York' ELSE 'Basingstoke' END,
       NOW()
FROM t;

-- A two-leg journey (KGX -> YRK, then YRK -> NCL): the multi-leg chaining
-- feature merged from journey Phase 2. Leg 1 is bound to the DELAYED train;
-- leg 2 is still an open window with no train picked yet, so the
-- "unmatched leg / pick a candidate" UI is also screenshottable.
WITH sub AS (
    INSERT INTO train_subscriptions
        (user_id, service_date, pin_origin_crs, pin_destination_crs,
         pin_scheduled_departure, pin_platform, pin_planned_platform,
         resolution_status, trains_id)
    VALUES ('demo-user', CURRENT_DATE, 'KGX', 'YRK',
            CURRENT_DATE + time '16:00', '1', '1',
            'resolved', (SELECT id FROM trains WHERE train_uid = 'P9E010'))
    RETURNING id
), j AS (
    INSERT INTO journeys (user_id, custom_name)
    VALUES ('demo-user', 'London to Newcastle (demo)')
    RETURNING id
)
INSERT INTO journey_legs
    (journey_id, leg_order, origin_crs, destination_crs, service_date,
     train_subscription_id, match_mode, depart_after, depart_before)
SELECT j.id, 1, 'KGX', 'YRK', CURRENT_DATE, sub.id, 'auto', NULL, NULL FROM j, sub
UNION ALL
SELECT j.id, 2, 'YRK', 'NCL', CURRENT_DATE, NULL, 'unmatched', time '19:00', time '21:00' FROM j;

-- A second, single-leg journey on a train running to time -- the "all good"
-- counterpart to the delayed one above.
WITH sub AS (
    INSERT INTO train_subscriptions
        (user_id, service_date, pin_origin_crs, pin_destination_crs,
         pin_scheduled_departure, pin_platform, pin_planned_platform,
         resolution_status, trains_id)
    VALUES ('demo-user', CURRENT_DATE, 'WAT', 'SOU',
            CURRENT_DATE + time '17:35', '11', '11',
            'resolved', (SELECT id FROM trains WHERE train_uid = 'P9S010'))
    RETURNING id
), j AS (
    INSERT INTO journeys (user_id, custom_name)
    VALUES ('demo-user', 'Waterloo to Southampton (demo)')
    RETURNING id
)
INSERT INTO journey_legs
    (journey_id, leg_order, origin_crs, destination_crs, service_date,
     train_subscription_id, match_mode)
SELECT j.id, 1, 'WAT', 'SOU', CURRENT_DATE, sub.id, 'manual' FROM j, sub;

COMMIT;

-- Quick visibility check.
\echo ''
\echo '--- seeded row counts ---'
SELECT 'tocs' AS t, count(*) FROM tocs
UNION ALL SELECT 'stations', count(*) FROM stations
UNION ALL SELECT 'line_status', count(*) FROM line_status
UNION ALL SELECT 'incidents', count(*) FROM incidents
UNION ALL SELECT 'schedule_departures_today', count(*) FROM schedule_destination_departures WHERE service_date = CURRENT_DATE
UNION ALL SELECT 'station_samples', count(*) FROM station_samples
UNION ALL SELECT 'journeys', count(*) FROM journeys
UNION ALL SELECT 'journey_legs', count(*) FROM journey_legs
UNION ALL SELECT 'train_subscriptions', count(*) FROM train_subscriptions;
