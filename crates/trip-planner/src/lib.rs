//! Journey-search algorithms over a `schedule_query::Connection` array and
//! `schedule_query::InterchangeData` -- Connection Scan (`csa`, Phase 3)
//! and RAPTOR (`raptor`, Phase 4), plus the differential test asserting
//! they agree; arrive-by (`reverse`), waypoints (`staged`), avoid lists
//! (`restrictions`) and pass-through vias (`via`). See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.

pub mod csa;
pub mod overlay;
pub mod raptor;
pub mod restrictions;
pub mod reverse;
pub mod staged;
pub mod via;

pub use csa::{
    Journey, JourneyLeg, ScanOptions, TrainLeg, TransferLeg, scan_connections,
    scan_connections_restricted, scan_connections_with_overlay,
};
pub use overlay::ConnectionOverlay;
pub use raptor::{
    RaptorJourney, RaptorOptions, raptor_search, raptor_search_restricted,
    raptor_search_with_overlay,
};
pub use restrictions::{PassLeg, Restrictions};
pub use reverse::{
    ArriveByOptions, latest_departure, latest_departures_by_trips, raptor_arrive_by,
    raptor_arrive_by_from_latest, scan_connections_arrive_by, staged_arrive_by,
    staged_raptor_arrive_by_from_latest,
};
pub use staged::{JourneyPart, StagedJourney, StagedOptions, raptor_staged, scan_staged};
pub use via::{PassSpan, ViaHow, ViaLeg, Vias};
