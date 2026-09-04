//! `movement-relay`: the sole real Kafka client against RDM's Train
//! Movements product from Deploy B onward, fanning out into the
//! `movement-events` Redis Stream both `trust-consumer` and
//! `full-coverage-consumer` read from. See
//! docs/superpowers/specs/2026-09-04-movement-relay-design.md and
//! docs/superpowers/plans/2026-09-04-movement-relay-plan.md.
//!
//! Scaffolding only at this stage (Task 5 of the plan above) -- the raw
//! Kafka source (Task 6) and the consume/classify/publish loop (Task 7)
//! land in later commits.

mod config;
mod health;

fn main() {
    unimplemented!(
        "wired up in Task 7 of docs/superpowers/plans/2026-09-04-movement-relay-plan.md"
    );
}
