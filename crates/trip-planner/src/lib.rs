//! Journey-search algorithms over a `schedule_query::Connection` array and
//! `schedule_query::InterchangeData` -- Connection Scan (`csa`, Phase 3)
//! and RAPTOR (`raptor`, Phase 4), plus the differential test asserting
//! they agree. See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.

pub mod csa;
pub mod overlay;
pub mod raptor;

pub use csa::{
    Journey, JourneyLeg, ScanOptions, TrainLeg, TransferLeg, scan_connections,
    scan_connections_with_overlay,
};
pub use overlay::ConnectionOverlay;
pub use raptor::{RaptorJourney, RaptorOptions, raptor_search, raptor_search_with_overlay};
