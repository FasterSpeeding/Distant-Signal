//! The TRUST event backlog: `trust_event_backlog.rs` and
//! `trust_event_backlog_match.rs` (whole), and the `train_reasons`
//! writers (`upsert_reasons`, `reason_text`, `reason_fields`).

/// A Postgres error caused by the row itself rather than by the database
/// or the connection -- see [`classify_data_error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError {
    pub sqlstate: String,
    pub reason: &'static str,
    pub constraint: Option<String>,
    pub message: String,
}

impl DataError {
    /// The wire shape a route reports this row's rejection in.
    pub fn into_rejected_row(
        self,
        index: usize,
        dedup_key: &str,
    ) -> common::RejectedTrustBacklogRow {
        common::RejectedTrustBacklogRow {
            index,
            dedup_key: dedup_key.to_string(),
            sqlstate: self.sqlstate,
            reason: self.reason.to_string(),
            constraint: self.constraint,
            message: self.message,
        }
    }
}

/// [`classify_data_error`] for an `anyhow::Error` from the data layer: walks
/// the error chain for the underlying `sqlx::Error` (or a
/// [`SharedMovementError`], which carries its classification with it).
/// `None` -- "transient, fail the request so the caller retries" -- for
/// anything else, including an error with no database cause at all.
pub fn classify_anyhow_data_error(err: &anyhow::Error) -> Option<DataError> {
    err.chain().find_map(|cause| {
        if let Some(sqlx_err) = cause.downcast_ref::<sqlx::Error>() {
            return classify_data_error(sqlx_err);
        }
        cause
            .downcast_ref::<SharedMovementError>()
            .and_then(|shared| shared.data_error.clone())
    })
}

/// One failure of a batched shared-movement step, reported against every
/// event that step covered. An `anyhow::Error` cannot be cloned, so the
/// fan-out keeps the message and -- what the route actually needs -- the
/// data-vs-transient classification of the original error (PL-7).
#[derive(Debug)]
pub struct SharedMovementError {
    message: String,
    data_error: Option<DataError>,
}

impl SharedMovementError {
    pub fn new(step: &str, err: &anyhow::Error) -> Self {
        Self {
            message: format!("{step} failed: {err:#}"),
            data_error: classify_anyhow_data_error(err),
        }
    }
}

impl std::fmt::Display for SharedMovementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SharedMovementError {}

/// `Some` only for a data error: SQLSTATE class 23 (integrity constraint
/// violation: check, not-null, unique, foreign-key, exclusion) or class 22
/// (data exception: invalid text representation, out-of-range value, a NUL
/// byte in text, and so on). Resending such a row can never succeed.
///
/// Everything else is `None` and must fail the request so the consumer
/// retries: connection and I/O errors, pool timeouts, class 40
/// (serialization failure, deadlock), class 57 (query canceled / statement
/// timeout, admin shutdown), 55P03 (lock timeout), class 53 (out of
/// memory/disk), and any SQLSTATE this function does not expect, such as a
/// class 42 schema mismatch during a rolling deploy.
pub fn classify_data_error(err: &sqlx::Error) -> Option<DataError> {
    let sqlx::Error::Database(db) = err else {
        return None;
    };
    let sqlstate = db.code()?.into_owned();
    let reason = data_error_reason(&sqlstate)?;
    Some(DataError {
        reason,
        constraint: db.constraint().map(str::to_string),
        message: db.message().to_string(),
        sqlstate,
    })
}

/// Maps a class 22/23 SQLSTATE to its Postgres condition name (from the
/// Postgres "Error Codes" appendix); `None` for any other class. Unlisted
/// codes in either class fall back to the class name, so the result is
/// always one of a fixed set -- it is used as a metric label.
fn data_error_reason(sqlstate: &str) -> Option<&'static str> {
    let reason = match sqlstate {
        "23000" => "integrity_constraint_violation",
        "23001" => "restrict_violation",
        "23502" => "not_null_violation",
        "23503" => "foreign_key_violation",
        "23505" => "unique_violation",
        "23514" => "check_violation",
        "23P01" => "exclusion_violation",
        "22001" => "string_data_right_truncation",
        "22003" => "numeric_value_out_of_range",
        "22007" => "invalid_datetime_format",
        "22008" => "datetime_field_overflow",
        "22021" => "character_not_in_repertoire",
        "22P02" => "invalid_text_representation",
        "22P05" => "untranslatable_character",
        code if code.starts_with("23") => "integrity_constraint_violation",
        code if code.starts_with("22") => "data_exception",
        _ => return None,
    };
    Some(reason)
}

#[cfg(test)]
mod classify_tests {
    use super::*;

    #[test]
    fn constraint_violations_and_invalid_input_are_data_errors() {
        assert_eq!(data_error_reason("23514"), Some("check_violation"));
        assert_eq!(data_error_reason("23502"), Some("not_null_violation"));
        assert_eq!(
            data_error_reason("22P02"),
            Some("invalid_text_representation")
        );
        assert_eq!(
            data_error_reason("22021"),
            Some("character_not_in_repertoire")
        );
        assert_eq!(
            data_error_reason("23999"),
            Some("integrity_constraint_violation")
        );
        assert_eq!(data_error_reason("22999"), Some("data_exception"));
    }

    #[test]
    fn transient_and_unexpected_sqlstates_are_not_data_errors() {
        for code in [
            "40001", // serialization_failure
            "40P01", // deadlock_detected
            "57014", // query_canceled (statement_timeout)
            "55P03", // lock_not_available (lock_timeout)
            "08006", // connection_failure
            "53300", // too_many_connections
            "42703", // undefined_column, e.g. mid rolling deploy
        ] {
            assert_eq!(data_error_reason(code), None, "{code}");
        }
    }

    #[test]
    fn non_database_errors_are_not_data_errors() {
        assert!(classify_data_error(&sqlx::Error::PoolTimedOut).is_none());
        assert!(classify_data_error(&sqlx::Error::Io(std::io::Error::other("reset"))).is_none());
    }

    /// The anyhow-level classifier the two ingest routes use: a pool
    /// timeout under any amount of context is transient, a plain error with
    /// no database cause is transient, and a fanned-out
    /// [`SharedMovementError`] keeps the classification it was built with.
    #[test]
    fn anyhow_errors_are_classified_through_their_chain() {
        let pool_timeout = anyhow::Error::from(sqlx::Error::PoolTimedOut).context("while writing");
        assert!(classify_anyhow_data_error(&pool_timeout).is_none());
        assert!(classify_anyhow_data_error(&anyhow::anyhow!("no database cause")).is_none());

        let transient = anyhow::Error::from(SharedMovementError::new(
            "find_or_create_train",
            &pool_timeout,
        ));
        assert!(classify_anyhow_data_error(&transient).is_none());

        let data = DataError {
            sqlstate: "23514".to_string(),
            reason: "check_violation",
            constraint: None,
            message: "bad".to_string(),
        };
        let fanned_out = anyhow::Error::from(SharedMovementError {
            message: "mark_train_resolved failed".to_string(),
            data_error: Some(data.clone()),
        });
        assert_eq!(classify_anyhow_data_error(&fanned_out), Some(data));
    }
}
