//! Database-gated tests for `line_train_summaries` and the routes reading
//! it, one throwaway database per test (`#[sqlx::test]`):
//!
//! - the writer: rows derived in the population's transaction, replaced
//!   per date, left alone on an identical re-publish, rewritten on a
//!   catalogue change, none for a population it cannot hold;
//! - parity: `/trains?view=summary` and `/timetable` give the same body
//!   from the table as from the population JSONB;
//! - the timetable's SQL page against its in-memory reference, every
//!   filter, and cursor paging end to end.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::data::line_train_summaries::{
    self as lts, LineStations, TimetableFilter, decode_population, derive_rows,
};
use crate::data::queries;

const LINE: &str = "test-line-tt";

fn date() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 1, 7).expect("date")
}

/// WAT - WOK - BSK - WEY.
fn line() -> common::LineDefinition {
    let toml = format!(
        r#"
        id = "{LINE}"
        name = "Timetable test line"
        mode = "rail"
        category = "main-line"
        operators = ["SW"]
        [[stations]]
        crs = "WAT"
        role = "terminus"
        [[stations]]
        crs = "WOK"
        role = "junction"
        [[stations]]
        crs = "BSK"
        role = "major"
        [[stations]]
        crs = "WEY"
        role = "terminus"
        "#
    );
    toml::from_str(&toml).expect("line definition")
}

/// A public call at `tiploc` (`arr`/`dep` `HH:MM`, `day` its day offset),
/// or a pass when both are `None`.
fn cp(tiploc: &str, arr: Option<&str>, dep: Option<&str>, day: u8) -> Value {
    let t = |v: Option<&str>| v.map(|t| format!("{t}:00"));
    json!({
        "tiploc": tiploc, "kind": "Intermediate",
        "booked_arrival": t(arr), "booked_departure": t(dep),
        "public_arrival": t(arr), "public_departure": t(dep),
        "is_half_minute_arrival": false, "is_half_minute_departure": false,
        "day_offset": day,
    })
}

fn entry(uid: &str, scope: &str, dir: &str, due: Option<(&str, u8)>, cps: &[Value]) -> Value {
    let mut e = json!({
        "uid": uid, "calling_points": cps, "operator_atoc": "SW", "train_status": "P",
        "scope": scope, "direction": dir, "run_first_crs": "WAT", "run_last_crs": "WEY",
    });
    if let Some((time, day)) = due {
        e["line_due"] = json!({"time": format!("{time}:00"), "day_offset": day});
    }
    e
}

/// A down train WAT `dep` - WOK - BSK - WEY, from and to the depot.
fn down(uid: &str, scope: &str, times: [&str; 4]) -> Value {
    let [wat, wok, bsk, wey] = times;
    entry(
        uid,
        scope,
        "down",
        Some((wat, 0)),
        &[
            cp("TTLDEP", None, Some(wat), 0),
            cp("TTLWAT", None, Some(wat), 0),
            cp("TTLWOK", Some(wok), Some(wok), 0),
            cp("TTLBSK", Some(bsk), Some(bsk), 0),
            cp("TTLWEY", Some(wey), None, 0),
            cp("TTLDEP", None, None, 0),
        ],
    )
}

/// The fixture population: own, shared and touch trains both ways, a
/// tie on time, a bus, a long run, an overnight run, a next-morning
/// train of the date and a train with no public call on the line.
fn population() -> Value {
    let mut bus = entry(
        "TT-S0830",
        "shared",
        "down",
        Some(("08:30", 0)),
        &[
            cp("TTLWAT", None, Some("08:30"), 0),
            cp("TTLWOK", Some("08:55"), Some("08:56"), 0),
            cp("TTLGLD", Some("09:20"), None, 0),
        ],
    );
    bus["train_status"] = json!("B");
    bus["operator_atoc"] = json!("XC");
    json!([
        down("TT-D0800B", "line", ["08:00", "08:25", "08:50", "10:30"]),
        down("TT-D0600", "line", ["06:00", "06:25", "06:50", "08:30"]),
        down("TT-D0800A", "line", ["08:00", "08:26", "08:51", "10:31"]),
        entry(
            "TT-U0910",
            "line",
            "up",
            Some(("09:10", 0)),
            &[
                cp("TTLWEY", None, Some("09:10"), 0),
                cp("TTLBSK", Some("10:00"), Some("10:01"), 0),
                cp("TTLWAT", Some("11:40"), None, 0),
            ],
        ),
        bus,
        entry(
            "TT-T0840",
            "touch",
            "down",
            Some(("08:40", 0)),
            &[cp("TTLWOK", Some("08:40"), Some("08:41"), 0)],
        ),
        // Six hours from the line's start to its end: still running at
        // 14:00, which a fixed six-hour look-back missed.
        down("TT-LONG", "line", ["06:05", "09:00", "12:00", "15:30"]),
        entry(
            "TT-NIGHT",
            "line",
            "down",
            Some(("23:30", 0)),
            &[
                cp("TTLWAT", None, Some("23:30"), 0),
                cp("TTLWOK", Some("23:55"), Some("23:56"), 0),
                cp("TTLBSK", Some("00:20"), Some("00:21"), 1),
                cp("TTLWEY", Some("01:30"), None, 1),
            ],
        ),
        entry(
            "TT-EARLY",
            "line",
            "up",
            Some(("00:15", 1)),
            &[
                cp("TTLWEY", None, Some("00:15"), 1),
                cp("TTLWAT", Some("02:00"), None, 1),
            ],
        ),
        entry(
            "TT-PASS",
            "line",
            "down",
            None,
            &[cp("TTLWAT", None, None, 0), cp("TTLWEY", None, None, 0)],
        ),
    ])
}

async fn seed_reference(pool: &PgPool) {
    for (tiploc, crs) in [
        ("TTLWAT", "WAT"),
        ("TTLWOK", "WOK"),
        ("TTLBSK", "BSK"),
        ("TTLWEY", "WEY"),
        ("TTLGLD", "GLD"),
        ("TTLDEP", "XTD"),
    ] {
        sqlx::query(
            "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) \
             VALUES ($1, $2, $1, '00000', 0)",
        )
        .bind(tiploc)
        .bind(crs)
        .execute(pool)
        .await
        .expect("seed crosswalk");
    }
    for (uid, delay) in [("TT-D0800A", 5), ("TT-LONG", 20), ("TT-NIGHT", 0)] {
        let (trains_id,): (i64,) = sqlx::query_as(
            "INSERT INTO trains (train_uid, service_date, train_id) VALUES ($1, $2, $1) \
             RETURNING id",
        )
        .bind(uid)
        .bind(date())
        .fetch_one(pool)
        .await
        .expect("seed trains");
        sqlx::query(
            "INSERT INTO train_current_state (trains_id, status, last_reported_location, \
                 delay_minutes, updated_at) VALUES ($1, 'en_route', 'WOK', $2, NOW())",
        )
        .bind(trains_id)
        .bind(delay)
        .execute(pool)
        .await
        .expect("seed train_current_state");
    }
    sqlx::query(
        "INSERT INTO schedule_services (service_date, uid, mode, train_status, \
             train_category, stp) VALUES ($1, 'TT-S0830', 'replacement_bus', 'B', 'BR', 'O')",
    )
    .bind(date())
    .execute(pool)
    .await
    .expect("seed schedule_services");
}

fn app(pool: &PgPool, lines: Vec<common::LineDefinition>) -> crate::app::App {
    let mut app = crate::test_support::inert_app(pool.clone());
    std::sync::Arc::get_mut(&mut app)
        .expect("a fresh app is unshared")
        .config
        .lines = crate::data::config::LineCatalogue(lines);
    app
}

async fn get(app: &crate::app::App, uri: &str) -> (StatusCode, Value) {
    let router = crate::app::Router::new()
        .nest("/public", crate::routes::public_router())
        .with_state(app.clone());
    let response = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, body)
}

async fn table_rows(pool: &PgPool, service_date: chrono::NaiveDate) -> Vec<(String, Value)> {
    sqlx::query_as(
        "SELECT uid, jsonb_build_object('scope', scope, 'direction', direction, \
             'due', due_minute, 'end', end_minute, 'operator', operator_atoc, \
             'status', train_status, 'origin', origin_crs, 'destination', destination_crs, \
             'stops', on_line_stops, 'hasScope', has_scope) \
         FROM line_train_summaries WHERE line_id = $1 AND service_date = $2 ORDER BY uid",
    )
    .bind(LINE)
    .bind(service_date)
    .fetch_all(pool)
    .await
    .expect("read rows")
}

async fn upsert(
    pool: &PgPool,
    line: &common::LineDefinition,
    service_date: chrono::NaiveDate,
    population: &Value,
) -> lts::WriteOutcome {
    lts::upsert_population_with_summaries(
        pool,
        Some(line),
        LINE,
        service_date,
        population.to_string().into_boxed_str(),
    )
    .await
    .expect("upsert")
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL (a role that can create databases)"]
async fn the_writer_derives_replaces_per_date_and_skips_identical_publishes(pool: PgPool) {
    seed_reference(&pool).await;
    let line = line();
    let other_date = date().succ_opt().expect("date");
    let population = population();

    let outcome = upsert(&pool, &line, date(), &population).await;
    assert_eq!(
        outcome,
        lts::WriteOutcome {
            population_changed: true,
            summaries_written: Some(10)
        }
    );
    upsert(&pool, &line, other_date, &population).await;
    let rows = table_rows(&pool, date()).await;
    assert_eq!(rows.len(), 10);
    let row = |uid: &str| {
        rows.iter()
            .find(|(u, _)| u == uid)
            .map(|(_, r)| r.clone())
            .expect(uid)
    };
    assert_eq!(
        row("TT-D0600"),
        json!({
            "scope": "line", "direction": "down", "due": 360, "end": 510,
            "operator": "SW", "status": "P", "origin": "WAT", "destination": "WEY",
            "stops": [
                {"crs": "WAT", "minute": 360, "arrival": 360},
                {"crs": "WOK", "minute": 385, "arrival": 385},
                {"crs": "BSK", "minute": 410, "arrival": 410},
                {"crs": "WEY", "minute": 510, "arrival": 510},
            ],
            "hasScope": true,
        })
    );
    // Off the line at GLD: the destination is the schedule's end, the
    // stops only the line's.
    assert_eq!(row("TT-S0830")["destination"], "GLD");
    assert_eq!(row("TT-S0830")["end"], 535);
    // Next morning of the service date: minutes past 1440.
    assert_eq!(row("TT-NIGHT")["end"], 1440 + 90);
    assert_eq!(row("TT-EARLY")["due"], 1440 + 15);
    // No public call on the line: no time, no stops.
    assert_eq!(row("TT-PASS")["due"], Value::Null);
    assert_eq!(row("TT-PASS")["stops"], json!([]));

    // An identical re-publish writes nothing.
    assert_eq!(
        upsert(&pool, &line, date(), &population).await,
        lts::WriteOutcome {
            population_changed: false,
            summaries_written: None
        }
    );

    // A changed population replaces that date's rows only.
    let mut changed = population.clone();
    changed.as_array_mut().expect("array").truncate(3);
    changed[0]["operator_atoc"] = json!("GW");
    let outcome = upsert(&pool, &line, date(), &changed).await;
    assert_eq!(outcome.summaries_written, Some(3));
    let rows = table_rows(&pool, date()).await;
    assert_eq!(
        rows.iter().map(|(u, _)| u.as_str()).collect::<Vec<_>>(),
        ["TT-D0600", "TT-D0800A", "TT-D0800B"]
    );
    assert_eq!(rows[2].1["operator"], "GW");
    assert_eq!(table_rows(&pool, other_date).await.len(), 10);

    // A catalogue change re-derives an unchanged population (WOK no
    // longer on the line).
    let mut shorter = line.clone();
    shorter.stations.retain(|s| s.crs != "WOK");
    let outcome = upsert(&pool, &shorter, date(), &changed).await;
    assert_eq!(
        outcome,
        lts::WriteOutcome {
            population_changed: false,
            summaries_written: Some(3)
        }
    );
    assert_eq!(
        table_rows(&pool, date()).await[0].1["stops"]
            .as_array()
            .expect("stops")
            .len(),
        3
    );

    // A population the table cannot hold: stored, with no rows (readers
    // fall back to it).
    let mut twice = changed.clone();
    twice
        .as_array_mut()
        .expect("array")
        .push(changed[0].clone());
    let outcome = upsert(&pool, &line, date(), &twice).await;
    assert_eq!(outcome.summaries_written, Some(0));
    assert!(table_rows(&pool, date()).await.is_empty());
    let stored = queries::get_schedule_line_population(&pool, LINE, date())
        .await
        .expect("read")
        .expect("stored");
    assert_eq!(
        serde_json::from_str::<Value>(&stored).expect("json"),
        twice
    );
    let odd = json!([{"uid": 7, "calling_points": []}]);
    assert_eq!(
        upsert(&pool, &line, date(), &odd).await.summaries_written,
        Some(0)
    );

    // `rebuild_summaries` (the backfill) derives a population stored
    // without rows, and leaves current rows alone.
    queries::upsert_schedule_line_population(&pool, LINE, date(), &population.to_string())
        .await
        .expect("plain upsert");
    assert!(table_rows(&pool, date()).await.is_empty());
    assert_eq!(
        lts::rebuild_summaries(&pool, Some(&line), LINE, date(), false)
            .await
            .expect("rebuild"),
        Some(Some(10))
    );
    assert_eq!(
        lts::rebuild_summaries(&pool, Some(&line), LINE, date(), false)
            .await
            .expect("rebuild"),
        Some(None)
    );
    assert_eq!(
        lts::rebuild_summaries(&pool, Some(&line), LINE, other_date.succ_opt().expect("d"), false)
            .await
            .expect("rebuild"),
        None
    );
}

/// The summary queries the line page (and a few edge cases) make.
const SUMMARY_QUERIES: &[&str] = &[
    "from=08:00&to=09:00",
    "from=08:00&to=10:00&direction=up",
    "from=08:00&to=09:00&at=08:15",
    // The long run, nine hours after it reached the line.
    "from=13:30&to=15:30&at=14:00",
    "from=23:00&to=01:00&at=24:30",
    "scope=all&from=08:00&to=09:00&limit=2",
    "scope=all",
    "at=09:00",
    "from=20:00",
    "direction=down&from=00:00&to=47:59",
];

/// The timetable queries.
const TIMETABLE_QUERIES: &[&str] = &[
    "",
    "limit=3",
    "dir=down&limit=2",
    "from=WOK",
    "from=WOK&to=WEY",
    "to=WAT",
    "from=BSK&to=WAT&scope=line",
    "scope=shared",
    "scope=all&at=08:30",
    "at=23:00",
    "after=480.TT-D0800A&limit=2",
];

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL (a role that can create databases)"]
async fn the_table_and_the_population_give_the_same_bodies(pool: PgPool) {
    seed_reference(&pool).await;
    let line = line();
    let app = app(&pool, vec![line.clone()]);
    queries::upsert_schedule_line_population(&pool, LINE, date(), &population().to_string())
        .await
        .expect("plain upsert");
    let date = date();
    let uris: Vec<String> = SUMMARY_QUERIES
        .iter()
        .map(|q| format!("/public/lines/{LINE}/trains?date={date}&view=summary&{q}"))
        .chain(
            TIMETABLE_QUERIES
                .iter()
                .map(|q| format!("/public/lines/{LINE}/timetable?date={date}&{q}")),
        )
        .collect();

    let mut from_population = Vec::new();
    for uri in &uris {
        let (status, body) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        from_population.push(body);
    }
    assert_eq!(
        lts::rebuild_summaries(&pool, Some(&line), LINE, date, false)
            .await
            .expect("rebuild"),
        Some(Some(10))
    );
    for (uri, expected) in uris.iter().zip(&from_population) {
        let (status, body) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(&body, expected, "{uri}");
    }

    // The bodies above are not trivially empty, and say what the line
    // page needs.
    let summary = &from_population[3];
    let running: Vec<&str> = summary["running"]
        .as_array()
        .expect("running")
        .iter()
        .map(|t| t["uid"].as_str().expect("uid"))
        .collect();
    assert_eq!(running, ["TT-LONG"], "the long run is running at 14:00");
    let night = &from_population[4];
    assert_eq!(night["running"][0]["uid"], "TT-NIGHT");
    assert_eq!(night["trains"][0]["uid"], "TT-NIGHT");
    let bus = &from_population[0]["trains"];
    assert_eq!(bus[2]["uid"], "TT-S0830");
    assert_eq!(bus[2]["serviceMode"], "replacementBus");

    // And the table really is what is read now: a marker written into it
    // shows up.
    sqlx::query(
        "UPDATE line_train_summaries SET operator_atoc = 'ZZ' \
         WHERE line_id = $1 AND uid = 'TT-D0800A'",
    )
    .bind(LINE)
    .execute(&pool)
    .await
    .expect("mark");
    let (_, body) = get(&app, &uris[0]).await;
    assert_eq!(body["trains"][0]["uid"], "TT-D0800A");
    assert_eq!(body["trains"][0]["operator"], "ZZ");
    // ... unless the catalogue changed since it was written: then the
    // population is read again.
    let mut moved = line.clone();
    moved.crs_aliases.insert("XYZ".into(), "WAT".into());
    let (_, body) = get(&app_with(&pool, moved), &uris[0]).await;
    assert_eq!(body["trains"][0]["operator"], "SW");
}

fn app_with(pool: &PgPool, line: common::LineDefinition) -> crate::app::App {
    app(pool, vec![line])
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL (a role that can create databases)"]
async fn the_timetable_pages_and_filters_like_its_reference(pool: PgPool) {
    seed_reference(&pool).await;
    let line = line();
    upsert(&pool, &line, date(), &population()).await;
    let fingerprint = lts::derivation_fingerprint(&LineStations::from_definition(&line));
    let decoded = decode_population(&population().to_string()).expect("decode");
    let has_scope = decoded.has_scope;
    let tiplocs: Vec<String> = ["TTLWAT", "TTLWOK", "TTLBSK", "TTLWEY", "TTLGLD", "TTLDEP"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let crosswalk = queries::crs_for_tiplocs_batch(&pool, &tiplocs)
        .await
        .expect("crosswalk");
    let rows = derive_rows(decoded, &crosswalk, &LineStations::from_definition(&line))
        .expect("rows");

    let s = |v: &[&str]| Some(v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>());
    let filters = [
        TimetableFilter {
            limit: 100,
            ..TimetableFilter::default()
        },
        TimetableFilter {
            scopes: s(&["line"]),
            limit: 100,
            ..TimetableFilter::default()
        },
        TimetableFilter {
            scopes: s(&["line", "shared"]),
            directions: s(&["down"]),
            at: Some(8 * 60),
            limit: 100,
            ..TimetableFilter::default()
        },
        TimetableFilter {
            from: Some("WOK".into()),
            to: Some("WEY".into()),
            limit: 100,
            ..TimetableFilter::default()
        },
        TimetableFilter {
            from: Some("BSK".into()),
            to: Some("WAT".into()),
            limit: 100,
            ..TimetableFilter::default()
        },
        TimetableFilter {
            to: Some("BSK".into()),
            scopes: s(&["line"]),
            limit: 100,
            ..TimetableFilter::default()
        },
        TimetableFilter {
            from: Some("GLD".into()),
            limit: 100,
            ..TimetableFilter::default()
        },
    ];
    for filter in &filters {
        let full = lts::timetable_page_in_memory(rows.clone(), has_scope, filter);
        let sql = lts::timetable_page(&pool, LINE, date(), &fingerprint, filter)
            .await
            .expect("page")
            .expect("rows exist");
        assert_eq!(sql, full, "{filter:?}");
        // Walk the same list two at a time: the pages concatenate to it.
        let mut walked = Vec::new();
        let mut paged = TimetableFilter {
            limit: 2,
            ..filter.clone()
        };
        loop {
            let page = lts::timetable_page(&pool, LINE, date(), &fingerprint, &paged)
                .await
                .expect("page")
                .expect("rows exist");
            assert_eq!(
                page,
                lts::timetable_page_in_memory(rows.clone(), has_scope, &paged),
                "{paged:?}"
            );
            assert!(page.entries.len() <= 2);
            walked.extend(page.entries.into_iter().map(|e| e.row.uid));
            match page.next {
                Some(next) => paged.after = Some(next),
                None => break,
            }
        }
        assert_eq!(
            walked,
            full.entries
                .iter()
                .map(|e| e.row.uid.clone())
                .collect::<Vec<_>>(),
            "{filter:?}"
        );
    }

    // What the filters mean, on the HTTP body.
    let app = app(&pool, vec![line]);
    let date = date();
    let uids = |body: &Value| -> Vec<String> {
        body["trains"]
            .as_array()
            .expect("trains")
            .iter()
            .map(|t| t["uid"].as_str().expect("uid").to_string())
            .collect()
    };
    let (status, body) = get(&app, &format!("/public/lines/{LINE}/timetable?date={date}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Default scope line,shared, by time on the line, then uid; the train
    // without an on-line call is not timetabled; the next-morning ones
    // last.
    assert_eq!(
        uids(&body),
        [
            "TT-D0600", "TT-LONG", "TT-D0800A", "TT-D0800B", "TT-S0830", "TT-U0910",
            "TT-NIGHT", "TT-EARLY"
        ]
    );
    assert_eq!(body["nextCursor"], Value::Null);
    assert_eq!(
        body["counts"],
        json!({"line": {"down": 5, "up": 2}, "shared": {"down": 1}})
    );
    assert_eq!(body["trains"][7]["time"], json!({"time": "00:15", "dayOffset": 1}));
    assert_eq!(body["trains"][2]["live"]["delayMinutes"], 5);
    assert_eq!(body["trains"][4]["serviceMode"], "replacementBus");
    assert_eq!(body["stations"][1]["role"], "junction");

    let (_, body) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date={date}&dir=down&limit=2"),
    )
    .await;
    assert_eq!(uids(&body), ["TT-D0600", "TT-LONG"]);
    let cursor = body["nextCursor"].as_str().expect("cursor").to_string();
    assert_eq!(cursor, "365.TT-LONG");
    let (_, body) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date={date}&dir=down&limit=2&after={cursor}"),
    )
    .await;
    assert_eq!(uids(&body), ["TT-D0800A", "TT-D0800B"]);
    // Counts are the whole day's, whatever the page and direction.
    assert_eq!(
        body["counts"],
        json!({"line": {"down": 5, "up": 2}, "shared": {"down": 1}})
    );

    // Between two stations: timed by the departure from `from` (so the
    // 08:00s swap places), with the arrival at `to`; the shared train
    // leaves the line before WEY. Only the up train runs BSK to WAT.
    let (_, body) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date={date}&from=wok&to=WEY"),
    )
    .await;
    assert_eq!(
        uids(&body),
        ["TT-D0600", "TT-D0800B", "TT-D0800A", "TT-LONG", "TT-NIGHT"]
    );
    assert_eq!(body["trains"][1]["time"], json!({"time": "08:25", "dayOffset": 0}));
    assert_eq!(body["trains"][1]["arrival"], json!({"time": "10:30", "dayOffset": 0}));
    assert_eq!(body["from"], "WOK");
    let (_, body) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date={date}&from=BSK&to=WAT"),
    )
    .await;
    assert_eq!(uids(&body), ["TT-U0910"]);
    let (_, body) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date={date}&scope=touch"),
    )
    .await;
    assert_eq!(uids(&body), ["TT-T0840"]);
    let (_, body) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date={date}&at=23:00"),
    )
    .await;
    assert_eq!(uids(&body), ["TT-NIGHT", "TT-EARLY"]);

    for bad in [
        "from=WOKING",
        "from=WOK&to=WOK",
        "after=junk",
        "limit=0",
        "dir=north",
        "at=7",
        "scope=nope",
    ] {
        let (status, _) = get(
            &app,
            &format!("/public/lines/{LINE}/timetable?date={date}&{bad}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (status, _) = get(
        &app,
        &format!("/public/lines/{LINE}/timetable?date=2026-01-09"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
