//! The timetable loop (transit spec §6.1-6.3). Plan task 2.1 wires the
//! loop; the GTFS adapter (2.2) and the `NaPTAN` enrichment, rail links,
//! feed guard and produce (2.3) fill in [`poll_once`].

use crate::Context;

/// One timetable cycle.
#[allow(clippy::unused_async, reason = "a stub until transit plan task 2.2")]
pub(crate) async fn poll_once(ctx: &Context) -> anyhow::Result<()> {
    anyhow::bail!(
        "network {}: the timetable adapter is not built yet (transit plan tasks 2.2 and 2.3)",
        ctx.network.id
    )
}
