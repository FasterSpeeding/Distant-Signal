//! A train's `delayMinutes` as a passenger sees it: measured against the
//! PUBLIC timetable at the passenger's own stop (design doc §9 decision 2).
//!
//! `train_current_state.delay_minutes` is TRUST's running delay, from the
//! train's latest report anywhere (a pass included), against the working
//! timetable. It stays internal: it is the input to the forecasts here and
//! to the per-stop estimates (`journey::apply_delay_estimates`). Every route
//! that serves a train-level `delayMinutes` replaces it through
//! [`apply_public_delays`] with:
//!
//! * the delay at the passenger's own stop (where they get off: a tracked
//!   train's pin destination, a journey leg's destination), measured once
//!   the train has reported there and forecast before then
//!   (`delayProvisional: true`); or
//! * with no stop of their own (the public train page, a line's trains),
//!   the delay at the latest call the train reported at.
//!
//! See `common::public_delay` for the arithmetic and the fallbacks, and
//! `delayBasis` for which baseline was used.

// Moved to ds_store::trains::stop_delay (ingest architecture plan 1A.4)
pub use ds_store::trains::stop_delay::{
    DelayBasis, PublicDelayFields, StopDelay, StopDelayTarget, apply_public_delays, split,
    stop_delays, target,
};
