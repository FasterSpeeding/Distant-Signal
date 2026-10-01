//! End-to-end tests of the ticket-parse child process (M13): every test here
//! spawns the real `api` binary as `api parse-ticket ...`, the same way
//! `routes::train` does in production.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api::data::ticket_extraction;
use api::data::ticket_precheck::fixtures;
use api::data::ticket_subprocess::{
    ChildLimits, ChildMode, ParseFailure, TicketKind, TicketParser,
};

fn api_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_api"))
}

fn parser(slots: usize, timeout: Duration, limits: ChildLimits) -> TicketParser {
    TicketParser::new(api_exe(), slots, timeout, limits).with_test_hooks()
}

fn default_parser() -> TicketParser {
    parser(2, Duration::from_secs(10), ChildLimits::default())
}

/// True once `pid` no longer exists (reaped). A zombie still has a
/// `/proc/<pid>` entry, so this also proves the parent waited for it.
fn process_is_gone(pid: u32) -> bool {
    !std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[tokio::test]
async fn a_pkpass_parsed_in_the_child_matches_the_in_process_parse() {
    let bytes = fixtures::train_pkpass();
    let expected = ticket_extraction::parse_pkpass(&bytes).expect("in-process parse");
    let got = default_parser()
        .parse(TicketKind::Pkpass, bytes)
        .await
        .expect("child parse");
    assert_eq!(got, expected);
    assert_eq!(got.source, "pkpass-semantics");
    assert!(got.current_departure_date.is_some());
}

#[tokio::test]
async fn a_pdf_parsed_in_the_child_matches_the_in_process_parse() {
    let bytes = fixtures::train_pdf();
    let expected = ticket_extraction::parse_pdf(&bytes).expect("in-process parse");
    assert_eq!(
        expected.ticket_type.as_deref(),
        Some("Super Off-Peak Return")
    );
    let got = default_parser()
        .parse(TicketKind::Pdf, bytes)
        .await
        .expect("child parse");
    assert_eq!(got, expected);
}

#[tokio::test]
async fn a_parser_error_in_the_child_comes_back_as_unparseable() {
    let bytes = fixtures::zip_with(&[(
        "pass.json",
        br#"{"boardingPass": {"transitType": "PKTransitTypeAir"}}"#,
    )]);
    let in_process = ticket_extraction::parse_pkpass(&bytes)
        .unwrap_err()
        .to_string();
    match default_parser().parse(TicketKind::Pkpass, bytes).await {
        Err(ParseFailure::Unparseable(msg)) => assert_eq!(msg, in_process),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_panic_in_the_child_is_caught_and_reported() {
    match default_parser().run(ChildMode::TestPanic, Vec::new()).await {
        Err(ParseFailure::Unparseable(msg)) => assert!(msg.contains("crashed"), "{msg}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_hanging_parse_is_killed_at_the_timeout_and_frees_its_slot() {
    let pids = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&pids);
    let parser = parser(1, Duration::from_millis(500), ChildLimits::default())
        .with_on_spawn(move |pid| seen.lock().unwrap().push(pid));

    let started = Instant::now();
    let result = parser.run(ChildMode::TestHang, Vec::new()).await;
    let elapsed = started.elapsed();

    assert!(matches!(result, Err(ParseFailure::TimedOut)), "{result:?}");
    assert!(
        elapsed >= Duration::from_millis(500) && elapsed < Duration::from_secs(5),
        "took {elapsed:?}"
    );
    assert_eq!(parser.available_slots(), 1, "the slot must be free again");
    let pid = pids.lock().unwrap()[0];
    assert!(
        process_is_gone(pid),
        "child {pid} must be killed and reaped"
    );

    // And the freed slot is usable.
    parser
        .parse(TicketKind::Pkpass, fixtures::train_pkpass())
        .await
        .expect("parse after timeout");
}

#[tokio::test]
async fn all_slots_busy_is_refused_immediately_and_dropping_a_request_kills_its_child() {
    let pids = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&pids);
    let parser = Arc::new(
        parser(2, Duration::from_secs(60), ChildLimits::default())
            .with_on_spawn(move |pid| seen.lock().unwrap().push(pid)),
    );

    let hangs: Vec<_> = (0..2)
        .map(|_| {
            let parser = Arc::clone(&parser);
            tokio::spawn(async move { parser.run(ChildMode::TestHang, Vec::new()).await })
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while parser.available_slots() > 0 || pids.lock().unwrap().len() < 2 {
        assert!(Instant::now() < deadline, "children never started");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let started = Instant::now();
    let busy = parser
        .parse(TicketKind::Pkpass, fixtures::train_pkpass())
        .await;
    assert!(matches!(busy, Err(ParseFailure::Busy)), "{busy:?}");
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "must not queue"
    );

    // A dropped request (client gone) releases its slot and its child is
    // SIGKILLed by `kill_on_drop`.
    for hang in &hangs {
        hang.abort();
    }
    for hang in hangs {
        let _ = hang.await;
    }
    assert_eq!(parser.available_slots(), 2);
    let pids = pids.lock().unwrap().clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    for pid in pids {
        // tokio reaps a killed-on-drop child in the background.
        while !process_is_gone(pid) {
            assert!(
                Instant::now() < deadline,
                "child {pid} survived its request"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

#[tokio::test]
async fn the_address_space_limit_kills_a_child_that_allocates_too_much() {
    let limits = ChildLimits {
        address_space_bytes: 128 * 1024 * 1024,
        ..ChildLimits::default()
    };
    let parser = parser(1, Duration::from_secs(10), limits);

    // Well under the limit: the child allocates, touches and replies.
    match parser
        .run(ChildMode::TestAlloc(16 * 1024 * 1024), Vec::new())
        .await
    {
        Err(ParseFailure::Unparseable(msg)) => assert!(msg.contains("allocated"), "{msg}"),
        other => panic!("{other:?}"),
    }
    // Over it: the allocation fails and the child aborts; the parent reports
    // it and keeps running.
    match parser
        .run(ChildMode::TestAlloc(1024 * 1024 * 1024), Vec::new())
        .await
    {
        Err(ParseFailure::ChildDied { status, stderr }) => {
            assert!(status.contains("signal"), "{status}; stderr: {stderr}");
            assert!(stderr.contains("memory allocation"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(parser.available_slots(), 1);
}

#[tokio::test]
async fn the_cpu_limit_kills_a_spinning_child_before_the_wall_clock_timeout() {
    let limits = ChildLimits {
        cpu_seconds: 1,
        ..ChildLimits::default()
    };
    let parser = parser(1, Duration::from_secs(30), limits);
    let started = Instant::now();
    match parser.run(ChildMode::TestSpin, Vec::new()).await {
        // SIGXCPU (24) at the soft limit.
        Err(ParseFailure::ChildDied { status, .. }) => {
            assert!(status.contains("signal"), "{status}");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn the_child_refuses_test_modes_without_the_hook_variable() {
    // No `with_test_hooks`: the child must refuse, exiting 2 without a reply.
    let parser = TicketParser::new(
        api_exe(),
        1,
        Duration::from_secs(10),
        ChildLimits::default(),
    );
    match parser.run(ChildMode::TestHang, Vec::new()).await {
        Err(ParseFailure::ChildDied { status, stderr }) => {
            assert_eq!(status, "exit code 2");
            assert!(stderr.contains("test modes are disabled"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
}

/// Not a correctness test: prints the per-upload overhead of the child
/// process against an in-process parse. Run with
/// `cargo test --release -p api --test ticket_parse_subprocess -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "latency report, not a check"]
async fn report_subprocess_latency_overhead() {
    const ROUNDS: u32 = 50;
    let parser = default_parser();
    for (name, kind, bytes) in [
        ("pkpass", TicketKind::Pkpass, fixtures::train_pkpass()),
        ("pdf", TicketKind::Pdf, fixtures::train_pdf()),
    ] {
        // Warm the page cache and the parser's own statics.
        parser.parse(kind, bytes.clone()).await.unwrap();

        let started = Instant::now();
        for _ in 0..ROUNDS {
            match kind {
                TicketKind::Pkpass => ticket_extraction::parse_pkpass(&bytes).map(drop).unwrap(),
                TicketKind::Pdf => ticket_extraction::parse_pdf(&bytes).map(drop).unwrap(),
            }
        }
        let in_process = started.elapsed() / ROUNDS;

        let started = Instant::now();
        for _ in 0..ROUNDS {
            parser.parse(kind, bytes.clone()).await.unwrap();
        }
        let child = started.elapsed() / ROUNDS;
        println!(
            "{name}: in-process {in_process:?}, child process {child:?}, overhead {:?}",
            child.saturating_sub(in_process)
        );
    }
}
