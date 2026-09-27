//! Runs ticket-file parsing in a killable, resource-limited child process
//! (M13, 2026-09-27).
//!
//! ## Why a process, not a thread
//!
//! `pdf_extract`/`lopdf` and the `zip` crate are synchronous and expose no
//! cancellation hook. On a `spawn_blocking` thread, a `tokio::time::timeout`
//! only stops the request from waiting; the thread keeps running the
//! abandoned parse, and an allocation failure inside it aborts the whole
//! API. A child process can be SIGKILLed at the deadline and has its own
//! address space, so neither failure reaches the server.
//!
//! ## How
//!
//! The parent re-executes its own binary (`std::env::current_exe()`, which
//! is `/usr/local/bin/api` in the image) as `api parse-ticket <pdf|pkpass>`.
//! `main.rs` intercepts that argument before clap, the tokio runtime, or any
//! configuration is touched, and calls [`child_main`]. So the image needs no
//! second binary, and the child starts single-threaded with nothing but the
//! parser code.
//!
//! - **Input/output:** the file goes to the child's stdin; the child answers
//!   with one JSON [`ChildReply`] on stdout. No temporary files, so this
//!   works under `readOnlyRootFilesystem` with no extra volume.
//! - **Environment:** cleared (`env_clear`), working directory `/`. The
//!   child never sees `DATABASE_URL`, `SSO_CLIENT_SECRET`, etc.
//! - **Limits:** set in `pre_exec` (between fork and exec, so they are in
//!   force before any of the child's own code runs): `RLIMIT_AS`,
//!   `RLIMIT_CPU`, `RLIMIT_CORE = 0`, `RLIMIT_FSIZE = 0` (no file writes;
//!   pipes are exempt) and `RLIMIT_NOFILE`. The child's `oom_score_adj` is
//!   raised to 1000 so that, if the pod's cgroup runs out of memory anyway,
//!   the kernel kills a parse child before the server. None of this needs a
//!   capability: lowering limits and raising `oom_score_adj` are
//!   unprivileged, and the chart's `RuntimeDefault` seccomp profile allows
//!   `fork`/`execve`/`setrlimit`.
//! - **Cancellation:** on the deadline the parent SIGKILLs and reaps the
//!   child before answering. `kill_on_drop(true)` covers the other exit:
//!   the request future being dropped (client disconnect) kills the child
//!   too.
//! - **Concurrency:** a semaphore of owned permits, one per live child,
//!   acquired with `try_acquire_owned` (full = [`ParseFailure::Busy`], a
//!   503). The permit is released only once the child has been reaped (or,
//!   on drop, SIGKILLed), so it now counts live processes, and a slot can no
//!   longer be held forever by a stuck parse.
//! - **Panics:** the child still wraps the parse in `catch_unwind` (and
//!   `parse_pdf` keeps its own), so a parser panic comes back as an
//!   ordinary parse error rather than a crash.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::ticket_extraction::{self, PartialTicket};
use super::ticket_precheck;

/// The hidden subcommand `main.rs` dispatches to [`child_main`].
pub const SUBCOMMAND: &str = "parse-ticket";

/// Test-only child modes (`hang`, `spin`, `alloc`, `panic`) are refused
/// unless this variable is set to `1` in the child's environment. The
/// production parent clears the child's environment and never sets it; a
/// [`TicketParser`] only does so after [`TicketParser::with_test_hooks`].
pub const TEST_HOOKS_ENV: &str = "DISTANT_SIGNAL_PARSE_TICKET_TEST_HOOKS";

/// Largest stdin the child accepts: the larger of the two per-kind upload
/// caps. The parent's pre-checks already enforce the per-kind cap.
const MAX_CHILD_INPUT_BYTES: usize = ticket_precheck::MAX_PKPASS_UPLOAD_BYTES;

/// A `PartialTicket` as JSON is a few hundred bytes. Anything past this is
/// not a reply this code wrote.
const MAX_REPLY_BYTES: u64 = 64 * 1024;

/// Kept only for logging a crashed child's last words.
const MAX_STDERR_BYTES: u64 = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketKind {
    Pkpass,
    Pdf,
}

impl TicketKind {
    fn arg(self) -> &'static str {
        match self {
            Self::Pkpass => "pkpass",
            Self::Pdf => "pdf",
        }
    }
}

/// What the child is asked to do. Only [`ChildMode::Parse`] is reachable
/// from a route; the rest exist so tests can drive the kill, slot and
/// resource-limit paths with a real process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildMode {
    Parse(TicketKind),
    #[doc(hidden)]
    TestHang,
    #[doc(hidden)]
    TestSpin,
    #[doc(hidden)]
    TestAlloc(u64),
    #[doc(hidden)]
    TestPanic,
}

impl ChildMode {
    fn args(self) -> Vec<String> {
        match self {
            Self::Parse(kind) => vec![kind.arg().to_string()],
            Self::TestHang => vec!["test-hang".to_string()],
            Self::TestSpin => vec!["test-spin".to_string()],
            Self::TestAlloc(n) => vec!["test-alloc".to_string(), n.to_string()],
            Self::TestPanic => vec!["test-panic".to_string()],
        }
    }

    fn from_args(args: &[OsString]) -> Option<Self> {
        let args: Vec<&str> = args.iter().map(|a| a.to_str()).collect::<Option<_>>()?;
        match args.as_slice() {
            ["pkpass"] => Some(Self::Parse(TicketKind::Pkpass)),
            ["pdf"] => Some(Self::Parse(TicketKind::Pdf)),
            ["test-hang"] => Some(Self::TestHang),
            ["test-spin"] => Some(Self::TestSpin),
            ["test-alloc", n] => n.parse().ok().map(Self::TestAlloc),
            ["test-panic"] => Some(Self::TestPanic),
            _ => None,
        }
    }

    fn is_test_hook(self) -> bool {
        !matches!(self, Self::Parse(_))
    }
}

/// The child's one line of stdout.
#[derive(Debug, Serialize, Deserialize)]
pub enum ChildReply {
    Ok(PartialTicket),
    /// The parser (or the child's own re-run of the pre-checks) rejected the
    /// file; the message is user-facing.
    Err(String),
}

/// Resource limits applied to each child in `pre_exec`.
#[derive(Debug, Clone, Copy)]
pub struct ChildLimits {
    /// `RLIMIT_AS`: virtual address space. The child's baseline (the mapped
    /// `api` binary, its shared libraries, one malloc arena and the main
    /// stack) is about 30 MiB; a real ticket parse adds a few MiB.
    pub address_space_bytes: u64,
    /// `RLIMIT_CPU` soft limit (SIGXCPU); the hard limit is one second
    /// higher (SIGKILL). A backstop behind the wall-clock timeout, for the
    /// case where the parent itself is too starved to act on it.
    pub cpu_seconds: u64,
    /// `RLIMIT_NOFILE`.
    pub open_files: u64,
}

impl Default for ChildLimits {
    fn default() -> Self {
        Self {
            // 256 MiB: about eight times what a real parse needs, and the
            // route's 4 concurrent children at this cap (1 GiB,
            // `routes::train::TICKET_PARSE_PERMITS`) fit comfortably under
            // the chart's 3 GiB api memory limit alongside the server's own
            // steady state. `oom_score_adj` covers the rest.
            address_space_bytes: 256 * 1024 * 1024,
            cpu_seconds: 12,
            open_files: 32,
        }
    }
}

/// Why a parse produced no ticket. `routes::train` maps each variant to a
/// status code.
#[derive(Debug)]
pub enum ParseFailure {
    /// Every slot has a live child: 503.
    Busy,
    /// The child ran and the parser rejected the file (or panicked, caught
    /// by `catch_unwind`): 422, with a user-facing message.
    Unparseable(String),
    /// The child hit the wall-clock budget and was killed: 504.
    TimedOut,
    /// The child died without replying (killed by `RLIMIT_AS`/`RLIMIT_CPU`,
    /// or any other crash). `status` is the child's exit status for logs.
    ChildDied { status: String, stderr: String },
    /// The child couldn't be started, or replied with something that isn't
    /// a [`ChildReply`]: 500.
    Internal(anyhow::Error),
}

/// Spawns parse children, bounded by a fixed number of slots.
pub struct TicketParser {
    exe: PathBuf,
    slots: Arc<tokio::sync::Semaphore>,
    timeout: Duration,
    limits: ChildLimits,
    test_hooks: bool,
    on_spawn: Option<Arc<dyn Fn(u32) + Send + Sync>>,
}

impl TicketParser {
    /// `exe` must be a build of the `api` binary (its `main` handles
    /// [`SUBCOMMAND`]).
    pub fn new(exe: PathBuf, slots: usize, timeout: Duration, limits: ChildLimits) -> Self {
        Self {
            exe,
            slots: Arc::new(tokio::sync::Semaphore::new(slots)),
            timeout,
            limits,
            test_hooks: false,
            on_spawn: None,
        }
    }

    /// Lets this parser's children run the `test-*` modes.
    #[doc(hidden)]
    pub fn with_test_hooks(mut self) -> Self {
        self.test_hooks = true;
        self
    }

    /// Called with each child's pid right after it is spawned.
    #[doc(hidden)]
    pub fn with_on_spawn(mut self, on_spawn: impl Fn(u32) + Send + Sync + 'static) -> Self {
        self.on_spawn = Some(Arc::new(on_spawn));
        self
    }

    /// Slots not currently held by a live child.
    pub fn available_slots(&self) -> usize {
        self.slots.available_permits()
    }

    /// Parses `bytes` as `kind` in a child process.
    pub async fn parse(
        &self,
        kind: TicketKind,
        bytes: Vec<u8>,
    ) -> Result<PartialTicket, ParseFailure> {
        self.run(ChildMode::Parse(kind), bytes).await
    }

    /// [`Self::parse`] with any [`ChildMode`], for tests.
    #[doc(hidden)]
    pub async fn run(
        &self,
        mode: ChildMode,
        bytes: Vec<u8>,
    ) -> Result<PartialTicket, ParseFailure> {
        let Ok(_permit) = Arc::clone(&self.slots).try_acquire_owned() else {
            return Err(ParseFailure::Busy);
        };

        let mut command = tokio::process::Command::new(&self.exe);
        command
            .arg(SUBCOMMAND)
            .args(mode.args())
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if self.test_hooks {
            command.env(TEST_HOOKS_ENV, "1");
        }
        let limits = self.limits;
        // SAFETY: `apply_child_limits` runs in the forked child before
        // exec and only makes async-signal-safe syscalls (getrlimit,
        // setrlimit, open, write, close); it allocates nothing and takes no
        // locks.
        unsafe {
            command.pre_exec(move || apply_child_limits(&limits));
        }

        let mut child = command.spawn().map_err(|err| {
            ParseFailure::Internal(anyhow::anyhow!("spawning parse child: {err}"))
        })?;
        if let (Some(on_spawn), Some(pid)) = (&self.on_spawn, child.id()) {
            on_spawn(pid);
        }

        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(ParseFailure::Internal(anyhow::anyhow!(
                "parse child is missing a stdio pipe"
            )));
        };

        let exchange = async {
            let write = async {
                // A child that rejects early closes stdin; EPIPE is expected
                // then and the reply says why.
                let _ = stdin.write_all(&bytes).await;
                drop(stdin);
            };
            let ((), reply, errors) = tokio::join!(
                write,
                read_capped(stdout, MAX_REPLY_BYTES),
                read_capped(stderr, MAX_STDERR_BYTES),
            );
            let status = child.wait().await;
            (reply, errors, status)
        };

        let (reply, errors, status) = match tokio::time::timeout(self.timeout, exchange).await {
            Ok(done) => done,
            Err(_elapsed) => {
                // SIGKILL and reap before the permit is released, so the
                // slot really is free when this returns.
                if let Err(err) = child.kill().await {
                    tracing::error!(error = ?err, "could not kill a timed-out parse child");
                }
                return Err(ParseFailure::TimedOut);
            }
        };

        let status = status.map_err(|err| {
            ParseFailure::Internal(anyhow::anyhow!("waiting for parse child: {err}"))
        })?;
        let stderr = match errors {
            Ok((errors, _)) => String::from_utf8_lossy(&errors).into_owned(),
            Err(err) => format!("<could not read stderr: {err}>"),
        };
        if !status.success() {
            return Err(ParseFailure::ChildDied {
                status: describe_status(status),
                stderr,
            });
        }
        let (reply, reply_truncated) = reply.map_err(|err| {
            ParseFailure::Internal(anyhow::anyhow!("reading parse child reply: {err}"))
        })?;
        if reply_truncated {
            return Err(ParseFailure::Internal(anyhow::anyhow!(
                "parse child reply is larger than {MAX_REPLY_BYTES} bytes"
            )));
        }
        match serde_json::from_slice::<ChildReply>(&reply) {
            Ok(ChildReply::Ok(ticket)) => Ok(ticket),
            Ok(ChildReply::Err(message)) => Err(ParseFailure::Unparseable(message)),
            Err(err) => Err(ParseFailure::Internal(anyhow::anyhow!(
                "parse child replied with something that isn't a ChildReply: {err}; stderr: {stderr}"
            ))),
        }
    }
}

/// Reads up to `cap` bytes, then keeps draining (and discarding) until EOF
/// so the child never blocks on a full pipe. The flag says whether anything
/// was discarded.
async fn read_capped(
    reader: impl tokio::io::AsyncRead + Unpin,
    cap: u64,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::new();
    let mut limited = reader.take(cap);
    limited.read_to_end(&mut kept).await?;
    let discarded = tokio::io::copy(&mut limited.into_inner(), &mut tokio::io::sink()).await?;
    Ok((kept, discarded > 0))
}

fn describe_status(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        (None, None) => status.to_string(),
    }
}

/// Runs in the forked child between `fork` and `exec`. Async-signal-safe
/// syscalls only: no allocation, no locks, no `std::fs`.
fn apply_child_limits(limits: &ChildLimits) -> std::io::Result<()> {
    set_limit(
        libc::RLIMIT_AS,
        limits.address_space_bytes,
        limits.address_space_bytes,
    )?;
    set_limit(libc::RLIMIT_CPU, limits.cpu_seconds, limits.cpu_seconds + 1)?;
    set_limit(libc::RLIMIT_CORE, 0, 0)?;
    set_limit(libc::RLIMIT_FSIZE, 0, 0)?;
    set_limit(libc::RLIMIT_NOFILE, limits.open_files, limits.open_files)?;
    raise_oom_score_adj();
    Ok(())
}

/// `setrlimit`'s resource parameter type differs between glibc and others.
#[cfg(target_env = "gnu")]
type Resource = libc::__rlimit_resource_t;
#[cfg(not(target_env = "gnu"))]
type Resource = libc::c_int;

/// Lowers a limit to (`soft`, `hard`), never above the current hard limit
/// (raising it would need `CAP_SYS_RESOURCE`, which the pod drops).
fn set_limit(resource: Resource, soft: u64, hard: u64) -> std::io::Result<()> {
    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: plain syscalls on a stack-allocated struct.
    unsafe {
        if libc::getrlimit(resource, &mut current) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let hard = (hard as libc::rlim_t).min(current.rlim_max);
        let soft = (soft as libc::rlim_t).min(hard);
        let wanted = libc::rlimit {
            rlim_cur: soft,
            rlim_max: hard,
        };
        if libc::setrlimit(resource, &wanted) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Best effort: on Linux, make this process the OOM killer's first choice
/// within the pod. Silently does nothing where `/proc` isn't available.
fn raise_oom_score_adj() {
    const PATH: &[u8] = b"/proc/self/oom_score_adj\0";
    const VALUE: &[u8] = b"1000";
    // SAFETY: open/write/close on a NUL-terminated static path and a static
    // buffer.
    unsafe {
        let fd = libc::open(PATH.as_ptr().cast(), libc::O_WRONLY | libc::O_CLOEXEC);
        if fd >= 0 {
            let _ = libc::write(fd, VALUE.as_ptr().cast(), VALUE.len());
            libc::close(fd);
        }
    }
}

/// If this process was started as `api parse-ticket ...`, runs the child
/// and returns its exit code; otherwise returns `None` and the caller
/// starts the server as usual. Must be called before anything else in
/// `main` (in particular before the tokio runtime and clap).
pub fn maybe_run_child() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(SUBCOMMAND)) {
        return None;
    }
    let rest: Vec<OsString> = args.collect();
    Some(child_main(&rest))
}

/// The child's whole life: read the file from stdin, run the pre-checks and
/// the parser under `catch_unwind`, write one [`ChildReply`] to stdout.
/// Exit code 0 means a reply was written (whether `Ok` or `Err`); 2 means
/// the command line was wrong.
pub fn child_main(args: &[OsString]) -> i32 {
    let Some(mode) = ChildMode::from_args(args) else {
        eprintln!("usage: api {SUBCOMMAND} <pdf|pkpass>  (file on stdin, JSON on stdout)");
        return 2;
    };
    if mode.is_test_hook() && std::env::var_os(TEST_HOOKS_ENV).as_deref() != Some("1".as_ref()) {
        eprintln!("{SUBCOMMAND}: test modes are disabled");
        return 2;
    }

    let mut bytes = Vec::new();
    let reply = match std::io::stdin()
        .lock()
        .take(MAX_CHILD_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
    {
        Err(err) => ChildReply::Err(format!("could not read the upload: {err}")),
        Ok(_) if bytes.len() > MAX_CHILD_INPUT_BYTES => ChildReply::Err(format!(
            "the upload is larger than {MAX_CHILD_INPUT_BYTES} bytes"
        )),
        Ok(_) => run_mode(mode, &bytes),
    };

    let mut stdout = std::io::stdout().lock();
    let written = serde_json::to_writer(&mut stdout, &reply)
        .map_err(std::io::Error::from)
        .and_then(|()| stdout.flush());
    match written {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{SUBCOMMAND}: could not write reply: {err}");
            1
        }
    }
}

fn run_mode(mode: ChildMode, bytes: &[u8]) -> ChildReply {
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<_> {
            match mode {
                ChildMode::Parse(TicketKind::Pkpass) => {
                    ticket_precheck::precheck_pkpass(bytes)?;
                    ticket_extraction::parse_pkpass(bytes)
                }
                ChildMode::Parse(TicketKind::Pdf) => {
                    ticket_precheck::precheck_pdf(bytes)?;
                    ticket_extraction::parse_pdf(bytes)
                }
                ChildMode::TestHang => loop {
                    std::thread::sleep(Duration::from_secs(3600));
                },
                ChildMode::TestSpin => {
                    let mut x: u64 = 0;
                    loop {
                        x = std::hint::black_box(x.wrapping_add(1));
                    }
                }
                ChildMode::TestAlloc(n) => {
                    // Touch every page so the allocation is real, not just
                    // reserved.
                    let buf = vec![1u8; usize::try_from(n)?];
                    anyhow::bail!("allocated {} bytes", std::hint::black_box(buf).len())
                }
                ChildMode::TestPanic => panic!("test-panic"),
            }
        }));
    match outcome {
        Ok(Ok(ticket)) => ChildReply::Ok(ticket),
        Ok(Err(err)) => ChildReply::Err(err.to_string()),
        Err(_) => ChildReply::Err("the ticket parser crashed on this file".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_modes_round_trip_through_their_arguments() {
        for mode in [
            ChildMode::Parse(TicketKind::Pdf),
            ChildMode::Parse(TicketKind::Pkpass),
            ChildMode::TestHang,
            ChildMode::TestSpin,
            ChildMode::TestAlloc(1234),
            ChildMode::TestPanic,
        ] {
            let args: Vec<OsString> = mode.args().into_iter().map(Into::into).collect();
            assert_eq!(ChildMode::from_args(&args), Some(mode));
        }
        assert_eq!(ChildMode::from_args(&["docx".into()]), None);
        assert_eq!(ChildMode::from_args(&[]), None);
    }

    #[test]
    fn test_modes_are_refused_without_the_hook_variable() {
        // This test process never sets TEST_HOOKS_ENV.
        assert!(std::env::var_os(TEST_HOOKS_ENV).is_none());
        assert_eq!(child_main(&["test-hang".into()]), 2);
    }

    #[test]
    fn a_parse_panic_becomes_an_error_reply() {
        match run_mode(ChildMode::TestPanic, b"") {
            ChildReply::Err(msg) => assert!(msg.contains("crashed"), "{msg}"),
            ChildReply::Ok(ticket) => panic!("{ticket:?}"),
        }
    }

    #[test]
    fn the_child_reruns_the_prechecks() {
        match run_mode(ChildMode::Parse(TicketKind::Pdf), b"not a pdf") {
            ChildReply::Err(msg) => assert!(msg.contains("not a PDF"), "{msg}"),
            ChildReply::Ok(ticket) => panic!("{ticket:?}"),
        }
    }

    #[test]
    fn a_reply_round_trips_through_json() {
        let ticket = ticket_extraction::parse_pkpass(&ticket_precheck::fixtures::train_pkpass())
            .expect("fixture parses");
        let json = serde_json::to_vec(&ChildReply::Ok(ticket.clone())).unwrap();
        match serde_json::from_slice::<ChildReply>(&json).unwrap() {
            ChildReply::Ok(back) => assert_eq!(back, ticket),
            ChildReply::Err(msg) => panic!("{msg}"),
        }
    }

    #[test]
    fn a_reply_with_an_unknown_source_is_refused() {
        let json = br#"{"Ok":{"operator":null,"ticketType":null,"originCrs":null,"destinationCrs":null,"source":"made-up"}}"#;
        assert!(serde_json::from_slice::<ChildReply>(json).is_err());
    }
}
