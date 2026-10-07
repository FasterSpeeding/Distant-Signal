//! The schedule publish protocol and products: `SchedulePublishPart`
//! (with `new` validating the chunk parameters), the publish-key SQL,
//! the destination-departure, calling-point, network-departure and
//! line-population upserts and readers, the feed-ingest and
//! reference-publish markers, and `schedule_feed_ingest_problem`
//! (spec §5.2).
//!
//! Filled by plan task 1A.7. The submodules live under `schedule/`; every
//! item is re-exported here, so callers name `ds_store::schedule::…`.
//!
//! - `markers`: the feed-ingest and reference-publish markers.

mod markers;

pub use markers::{
    ScheduleFeedSource, insert_schedule_feed_ingest, insert_schedule_reference_publish,
    last_completed_schedule_reference_publish, last_schedule_feed_fetch,
};
