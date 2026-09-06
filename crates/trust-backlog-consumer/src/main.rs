// Scaffold-only placeholder (Task 7 of
// docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md).
// Task 10 replaces this file wholesale with the real main loop; this
// placeholder exists only so `cargo check -p trust-backlog-consumer` has
// a target to parse at all -- a package with no src/main.rs and no
// [lib]/[[bin]] section fails to load its manifest entirely (confirmed
// directly: the plan's own Task 7 Step 4 assumes a `cargo check` here
// already produces "missing mod stanox_crs" errors, which requires SOME
// binary target to exist first). `mod config;` alone is enough to
// surface config.rs's own `crate::stanox_crs::StanoxCrsTable` reference
// as an unresolved-module error, exactly the "parses as valid Rust
// syntax, fails on the not-yet-written sibling modules" signal this task
// step is checking for.
mod config;
mod crs_index;
mod stanox_crs;

fn main() {}
