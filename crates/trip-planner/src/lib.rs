//! Journey-search algorithms over a `schedule_query::Connection` array and
//! `schedule_query::InterchangeData` -- Connection Scan (this phase) and,
//! from Phase 4 onward, RAPTOR, plus the differential test asserting they
//! agree. See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.

pub mod csa;

pub use csa::{Journey, JourneyLeg, ScanOptions, TrainLeg, TransferLeg, scan_connections};
