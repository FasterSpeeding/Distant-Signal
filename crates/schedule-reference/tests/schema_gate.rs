//! The schema gate (spec §12.2) passes for the `schedule_reference` role's
//! grants on a migrated database, as whichever role `DATABASE_URL` is (CI:
//! the superuser, the role-split app role, and in the per-service step the
//! narrowed `distant_signal_schedule_reference` itself, plan 2a.6).

use std::time::Duration;

use ds_store::schema::{DbRole, SchemaGate, wait_for_schema_with};

#[tokio::test]
#[ignore = "requires a live database; run with DATABASE_URL set and --ignored"]
async fn the_schema_gate_passes_for_the_schedule_reference_role() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let gate = SchemaGate {
        deadline: Duration::ZERO,
        ..SchemaGate::for_role(DbRole::ScheduleReference)
    };
    wait_for_schema_with(&pool, &gate, None).await.unwrap();
}
