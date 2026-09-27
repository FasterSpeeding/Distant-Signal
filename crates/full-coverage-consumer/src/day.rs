//! One rail day's in-memory correlation state, and the dispatch of raw
//! `movement-events` payloads into it. Shared by the live consume path and
//! the startup replay (`replay.rs`), so both apply an event identically.

use std::collections::{HashMap, HashSet};

use trust_schema::schema::TrustMessage;

use crate::correlate;
use crate::population::Population;
use crate::stanox_tiploc::StanoxTable;
use crate::station_correlate;

/// The STANOX/TIPLOC lookups every Movement is resolved through, rebuilt
/// on each stanox/crs reload.
#[derive(Debug, Default)]
pub struct Lookups {
    pub stanox: StanoxTable,
    /// TIPLOC -> every shadow line whose catalogue resolves to it.
    pub tiploc_index: HashMap<String, Vec<String>>,
}

/// Why a whole rail day is `partial` -- see [`DayState::partial_reason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialReason {
    /// The process started mid-day and the day's first events had already
    /// been trimmed from `movement-events` (or the replay could not tell).
    DayStartTrimmed,
    /// The process started mid-day on a backend with no replay (Kafka).
    ReplayUnsupported,
}

impl PartialReason {
    pub fn as_str(self) -> &'static str {
        match self {
            PartialReason::DayStartTrimmed => "day_start_trimmed",
            PartialReason::ReplayUnsupported => "replay_unsupported",
        }
    }
}

/// Everything this process knows about `service_date`'s rail day.
#[derive(Debug)]
pub struct DayState {
    pub service_date: chrono::NaiveDate,
    pub correlation: correlate::CorrelationState,
    pub stations: station_correlate::StationCorrelationState,
    /// Set when this process cannot have seen every event of the day, so
    /// "no event seen for a scheduled train" means nothing: the day's rows
    /// then leave unseen trains out instead of counting them as cancelled,
    /// and never read "available". Only ever set for the day a process
    /// STARTS in; a day entered by an in-process rollover saw everything
    /// from its first event.
    pub partial_reason: Option<PartialReason>,
    /// Lines whose population for this day was still missing (every fetch
    /// failing) when consumption began -- their early movements could not
    /// be matched, so their rows are partial for the rest of the day too.
    pub partial_lines: HashSet<String>,
    /// `train_id -> uid` of every Activation for the NEXT service date
    /// seen while this day is current. TRUST activates a train about an
    /// hour before it departs, so the next day's first trains (02:00-03:00
    /// London) are activated before this day closes -- 267 of them on
    /// 2026-09-27 -- and used to be wiped with this day's state at the
    /// rollover, so none of their movements could be matched. [`DayState::roll`]
    /// carries them into the next day.
    pub next_activations: HashMap<String, String>,
}

impl DayState {
    pub fn new(service_date: chrono::NaiveDate) -> Self {
        Self {
            service_date,
            correlation: correlate::CorrelationState::default(),
            stations: station_correlate::StationCorrelationState::default(),
            partial_reason: None,
            partial_lines: HashSet::new(),
            next_activations: HashMap::new(),
        }
    }

    /// The day after this one closes: a fresh state for `next`, keeping
    /// only the Activations already seen for it (see
    /// [`DayState::next_activations`]).
    pub fn roll(self, next: chrono::NaiveDate) -> DayState {
        let mut day = DayState::new(next);
        if next == self.service_date + chrono::Duration::days(1) {
            day.correlation.pending_activations = self.next_activations;
        }
        day
    }

    pub fn is_line_partial(&self, line_id: &str) -> bool {
        self.partial_reason.is_some() || self.partial_lines.contains(line_id)
    }

    /// Parses one raw stream payload and dispatches every message in it.
    /// `Err` only for an unparseable payload (the caller decides whether to
    /// dead-letter it).
    pub fn dispatch_payload(
        &mut self,
        raw: &str,
        lookups: &Lookups,
        population: &Population,
    ) -> anyhow::Result<()> {
        for message in trust_schema::schema::parse_batch(raw)? {
            if let TrustMessage::Activation(activation) = &message {
                self.note_next_day_activation(activation);
            }
            dispatch_message(
                message,
                &mut self.correlation,
                &mut self.stations,
                &lookups.stanox,
                &lookups.tiploc_index,
                population,
                self.service_date,
            );
        }
        Ok(())
    }
}

impl DayState {
    fn note_next_day_activation(&mut self, activation: &trust_schema::schema::Activation) {
        let next = self.service_date + chrono::Duration::days(1);
        if correlate::activation_service_date(activation, &[self.service_date, next]) == Some(next)
        {
            self.next_activations
                .insert(activation.train_id.clone(), activation.train_uid.clone());
        }
    }

    /// The startup replay's lookback segment (entries from before the rail
    /// day started, see `replay`): only Activations for THIS service date
    /// are applied, so a train activated before 02:00 London -- about an
    /// hour before it departs -- can still be matched. Nothing else from
    /// before the day start belongs to this day's rows.
    pub fn dispatch_lookback_payload(&mut self, raw: &str) -> anyhow::Result<()> {
        let previous = self.service_date - chrono::Duration::days(1);
        for message in trust_schema::schema::parse_batch(raw)? {
            if let TrustMessage::Activation(activation) = message
                && correlate::activation_service_date(&activation, &[previous, self.service_date])
                    == Some(self.service_date)
            {
                correlate::apply_activation(&mut self.correlation, &activation);
            }
        }
        Ok(())
    }
}

/// Dispatches one parsed `TrustMessage` into both running correlation
/// records. `ChangeOfOrigin`/`ChangeOfIdentity`/`Reinstatement`/`Unknown`
/// are deliberately ignored -- `correlate.rs`'s own scope (Decision 2d) only
/// covers Activation/Movement/Cancellation, the same three message types
/// `trust-consumer` itself keys real behaviour on. `Reinstatement` (`0005`,
/// confirmed by the H4 fix of the 2026-09-26 review) doesn't regress this
/// consumer's own "cancelled" line-level state by being ignored here: this
/// module's `apply_movement`/`apply_cancellation` already reuse
/// `trust_schema::journey` directly, so that fix's own `status_rank` change
/// (a fresh Movement can un-stick a `"cancelled"` per-line status without
/// needing a Reinstatement message specifically) already applies here too.
pub fn dispatch_message(
    message: TrustMessage,
    correlation_state: &mut correlate::CorrelationState,
    station_state: &mut station_correlate::StationCorrelationState,
    stanox: &StanoxTable,
    tiploc_index: &HashMap<String, Vec<String>>,
    population: &Population,
    service_date: chrono::NaiveDate,
) {
    match message {
        TrustMessage::Activation(activation) => {
            correlate::apply_activation(correlation_state, &activation);
            // `toc_id` is `Option` as of the 2026-09-25 review's finding #7
            // (a required field with no reader made one absent value drop the
            // WHOLE Activation, costing its far more valuable
            // train_id/train_uid binding). `None` simply means this uid
            // learns no operator here -- `station_correlate` already treats
            // "a UID absent from `activations_by_uid`" as a first-class case
            // and skips station correlation for it, exactly as it does for a
            // Movement whose Activation this process never saw.
            if let Some(toc_id) = activation.toc_id.as_deref() {
                station_correlate::apply_activation(station_state, &activation.train_uid, toc_id);
            } else {
                tracing::debug!(
                    train_uid = %activation.train_uid,
                    "Activation carries no toc_id; skipping station correlation for this uid"
                );
            }
        }
        TrustMessage::Movement(movement) => {
            let result = correlate::apply_movement(
                correlation_state,
                &movement,
                stanox,
                tiploc_index,
                population,
                service_date,
            );
            for (line_id, uid) in &result.matched_lines {
                metrics::counter!(
                    common::metrics::metric_name("full_coverage_consumer_events_matched_total"),
                    "line_id" => line_id.clone()
                )
                .increment(1);

                let Some(crs) = result.loc_crs.as_deref() else {
                    continue;
                };
                let Some(derived) = correlation_state
                    .derived
                    .get(&(line_id.clone(), uid.clone()))
                else {
                    continue;
                };
                let matched_station = station_correlate::apply_movement_station(
                    station_state,
                    &result.train_uid,
                    crs,
                    derived,
                );
                if !matched_station {
                    metrics::counter!(common::metrics::metric_name(
                        "full_coverage_consumer_station_buckets_dropped_total"
                    ))
                    .increment(1);
                }
            }
        }
        TrustMessage::Cancellation(cancellation) => {
            correlate::apply_cancellation(
                correlation_state,
                &cancellation,
                population,
                service_date,
            );
        }
        TrustMessage::ChangeOfOrigin(_)
        | TrustMessage::ChangeOfIdentity(_)
        | TrustMessage::Reinstatement(_)
        | TrustMessage::Unknown(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activation_payload(train_id: &str, uid: &str, origin_date: Option<&str>) -> String {
        let origin = origin_date
            .map(|d| format!(r#","tp_origin_timestamp":"{d}""#))
            .unwrap_or_default();
        format!(
            r#"{{"header":{{"msg_type":"0001"}},"body":{{"train_id":"{train_id}","train_uid":"{uid}","toc_id":"SW"{origin}}}}}"#
        )
    }

    /// Regression test for the lost pre-rollover activations: an
    /// Activation for D + 1 received while D is still current survives
    /// the rollover; one for D does not leak into D + 1.
    #[test]
    fn an_activation_for_the_next_day_survives_the_rollover() {
        let d: chrono::NaiveDate = "2026-09-26".parse().unwrap();
        let next = d + chrono::Duration::days(1);
        let mut day = DayState::new(d);
        let lookups = Lookups::default();
        let population = Population::default();
        // 00:30Z on the 27th: the 02:30 London departure is activated.
        day.dispatch_payload(
            &activation_payload("722N71MW27", "C11052", Some("2026-09-27")),
            &lookups,
            &population,
        )
        .unwrap();
        // A train of D, by its train_id digits alone.
        day.dispatch_payload(
            &activation_payload("722N72MW26", "C22222", None),
            &lookups,
            &population,
        )
        .unwrap();

        let rolled = day.roll(next);
        assert_eq!(rolled.service_date, next);
        assert_eq!(
            rolled
                .correlation
                .pending_activations
                .get("722N71MW27")
                .map(String::as_str),
            Some("C11052")
        );
        assert!(
            !rolled
                .correlation
                .pending_activations
                .contains_key("722N72MW26")
        );
        assert!(rolled.next_activations.is_empty());
    }

    #[test]
    fn the_lookback_applies_only_this_days_activations() {
        let d: chrono::NaiveDate = "2026-09-27".parse().unwrap();
        let mut day = DayState::new(d);
        day.dispatch_lookback_payload(&activation_payload("722N71MW27", "C11052", None))
            .unwrap();
        day.dispatch_lookback_payload(&activation_payload("722N72MW26", "C22222", None))
            .unwrap();
        day.dispatch_lookback_payload(
            r#"{"header":{"msg_type":"0003"},"body":{"train_id":"722N71MW27","event_type":"DEPARTURE","loc_stanox":"1","variation_status":"ON TIME"}}"#,
        )
        .unwrap();
        assert_eq!(
            day.correlation
                .pending_activations
                .keys()
                .collect::<Vec<_>>(),
            vec!["722N71MW27"]
        );
        assert!(day.correlation.derived.is_empty());
    }
}
