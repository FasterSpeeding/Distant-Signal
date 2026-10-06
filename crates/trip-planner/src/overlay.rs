//! A per-request replacement of some trains' connections, applied on top of
//! the shared, cached whole-day connections array without copying it.
//!
//! `/Trips/plan`'s live overlay (`api::data::trip_plan_live`) knows, for a
//! handful of trains, that the timetable is wrong today: a train is
//! cancelled (all of it, or from/to some stop), skips a stop, or runs late.
//! It expresses that as a [`ConnectionOverlay`]: the set of train UIDs whose
//! timetabled connections are withdrawn, plus the connections that replace
//! them (possibly none, for a fully cancelled train). The searches then walk
//! [`connections_from`]'s merge of the two -- the base array minus the
//! replaced UIDs, and the replacements -- in the same
//! `(departure_min, uid, from_tiploc)` order `build_connections` sorts by.
//!
//! The base array is ~100 MB per day and shared through a cache, so the
//! alternative (clone, edit, re-sort per request) would cost far more than
//! the search itself.
//!
//! [`connections_from`] also starts at the first connection departing at or
//! after the search's own departure time. No connection departing earlier
//! can ever be boarded -- every ready time in both searches is at least the
//! search's `departure_min` -- so this changes no result, only how much of
//! the day's array is walked.

use std::collections::HashSet;

use schedule_query::Connection;

/// See the module doc.
#[derive(Debug, Clone, Default)]
pub struct ConnectionOverlay {
    replaced_uids: HashSet<String>,
    replacements: Vec<Connection>,
}

fn order_key(connection: &Connection) -> (u32, &str, &str) {
    (
        connection.departure_min,
        connection.uid.as_str(),
        connection.from_tiploc.as_str(),
    )
}

impl ConnectionOverlay {
    /// `replaced_uids`: every train whose base connections are withdrawn.
    /// `replacements`: their new connections, in any order (sorted here).
    /// A replacement whose UID is not in `replaced_uids` is added as well,
    /// but the base array's own connections for that UID are kept too, so
    /// callers should always list the UID.
    pub fn new(
        replaced_uids: impl IntoIterator<Item = String>,
        mut replacements: Vec<Connection>,
    ) -> Self {
        replacements.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        Self {
            replaced_uids: replaced_uids.into_iter().collect(),
            replacements,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.replaced_uids.is_empty() && self.replacements.is_empty()
    }

    pub fn replaced_uids(&self) -> &HashSet<String> {
        &self.replaced_uids
    }

    pub fn replacements(&self) -> &[Connection] {
        &self.replacements
    }
}

/// `base` (sorted, as `build_connections` returns it) merged with
/// `overlay`, from the first connection departing at or after `from_min`.
/// See the module doc.
pub fn connections_from<'a>(
    base: &'a [Connection],
    overlay: Option<&'a ConnectionOverlay>,
    from_min: u32,
) -> MergedConnections<'a> {
    let start = base.partition_point(|c| c.departure_min < from_min);
    let (replaced, extra): (Option<&HashSet<String>>, &[Connection]) = match overlay {
        Some(overlay) if !overlay.is_empty() => {
            let extra_start = overlay
                .replacements
                .partition_point(|c| c.departure_min < from_min);
            (
                Some(&overlay.replaced_uids),
                &overlay.replacements[extra_start..],
            )
        }
        _ => (None, &[]),
    };
    MergedConnections {
        base: base[start..].iter(),
        replaced,
        extra: extra.iter(),
        next_base: None,
        next_extra: None,
    }
}

/// The iterator [`connections_from`] returns.
pub struct MergedConnections<'a> {
    base: std::slice::Iter<'a, Connection>,
    replaced: Option<&'a HashSet<String>>,
    extra: std::slice::Iter<'a, Connection>,
    next_base: Option<&'a Connection>,
    next_extra: Option<&'a Connection>,
}

impl<'a> MergedConnections<'a> {
    fn pull_base(&mut self) -> Option<&'a Connection> {
        match self.replaced {
            None => self.base.next(),
            Some(replaced) => self.base.find(|c| !replaced.contains(&c.uid)),
        }
    }
}

impl<'a> Iterator for MergedConnections<'a> {
    type Item = &'a Connection;

    fn next(&mut self) -> Option<&'a Connection> {
        if self.replaced.is_none() {
            // No overlay: a plain slice walk.
            return self.base.next();
        }
        if self.next_base.is_none() {
            self.next_base = self.pull_base();
        }
        if self.next_extra.is_none() {
            self.next_extra = self.extra.next();
        }
        match (self.next_base, self.next_extra) {
            (Some(b), Some(e)) => {
                if order_key(e) < order_key(b) {
                    self.next_extra.take()
                } else {
                    self.next_base.take()
                }
            }
            (Some(_), None) => self.next_base.take(),
            (None, Some(_)) => self.next_extra.take(),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
        Connection {
            uid: uid.to_string(),
            from_tiploc: from.to_string(),
            to_tiploc: to.to_string(),
            departure_min: dep,
            arrival_min: arr,
            working_departure_min: dep,
            working_arrival_min: arr,
            can_board: true,
            can_alight: true,
        }
    }

    fn uids<'a>(it: impl Iterator<Item = &'a Connection>) -> Vec<(String, u32)> {
        it.map(|c| (c.uid.clone(), c.departure_min)).collect()
    }

    #[test]
    fn without_an_overlay_it_is_the_base_from_the_departure_time() {
        let base = vec![
            conn("A", "X", "Y", 10, 20),
            conn("B", "X", "Y", 30, 40),
            conn("C", "X", "Y", 50, 60),
        ];
        assert_eq!(
            uids(connections_from(&base, None, 30)),
            vec![("B".to_string(), 30), ("C".to_string(), 50)]
        );
        let empty = ConnectionOverlay::default();
        assert_eq!(uids(connections_from(&base, Some(&empty), 0)).len(), 3);
    }

    #[test]
    fn replaced_trains_are_withdrawn_and_replacements_merged_in_order() {
        let base = vec![
            conn("A", "X", "Y", 10, 20),
            conn("B", "X", "Y", 30, 40),
            conn("B", "Y", "Z", 41, 50),
            conn("C", "X", "Y", 50, 60),
        ];
        // B runs 15 late; D (not in base) is ignored unless listed... it is
        // added regardless, as documented.
        let overlay = ConnectionOverlay::new(
            ["B".to_string()],
            vec![conn("B", "Y", "Z", 56, 65), conn("B", "X", "Y", 45, 55)],
        );
        assert_eq!(
            uids(connections_from(&base, Some(&overlay), 0)),
            vec![
                ("A".to_string(), 10),
                ("B".to_string(), 45),
                ("C".to_string(), 50),
                ("B".to_string(), 56),
            ]
        );
        // From 46: the replacement departing at 45 is skipped too.
        assert_eq!(
            uids(connections_from(&base, Some(&overlay), 46)),
            vec![("C".to_string(), 50), ("B".to_string(), 56)]
        );
    }

    /// Both searches honour an overlay: `U1` running 15 late misses the
    /// 09:00 `U2` at `B` (5-minute change), so both answer with `U3`.
    #[test]
    fn a_delay_in_the_overlay_moves_both_searches_onto_a_later_connection() {
        use schedule_query::InterchangeData;
        use std::collections::HashMap;

        let base = vec![
            conn("U1", "A", "B", 480, 530),
            conn("U2", "B", "C", 540, 600),
            conn("U3", "B", "C", 560, 620),
        ];
        let interchange = InterchangeData {
            modal_change: schedule_query::ModalChangeBuffer::default(),
            change_time_by_tiploc: HashMap::from([("B".to_string(), 5)]),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        let from = ["A".to_string()];
        let to = ["C".to_string()];
        let overlay =
            ConnectionOverlay::new(["U1".to_string()], vec![conn("U1", "A", "B", 495, 545)]);

        let scan = |overlay| {
            crate::scan_connections_with_overlay(
                crate::ScanOptions {
                    connections: &base,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    to_tiplocs: &to,
                    departure_min: 470,
                    date,
                },
                overlay,
            )
            .expect("a journey exists")
            .arrival_min
        };
        assert_eq!(scan(None), 600);
        assert_eq!(scan(Some(&overlay)), 620);

        let raptor = |overlay| {
            crate::raptor_search_with_overlay(
                crate::RaptorOptions {
                    connections: &base,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    to_tiplocs: &to,
                    departure_min: 470,
                    date,
                    max_rounds: 4,
                },
                overlay,
            )
            .iter()
            .map(|j| j.arrival_min)
            .min()
            .expect("a journey exists")
        };
        assert_eq!(raptor(None), 600);
        assert_eq!(raptor(Some(&overlay)), 620);
    }

    #[test]
    fn a_fully_cancelled_train_has_no_replacements() {
        let base = vec![conn("A", "X", "Y", 10, 20), conn("B", "X", "Y", 30, 40)];
        let overlay = ConnectionOverlay::new(["A".to_string()], Vec::new());
        assert_eq!(
            uids(connections_from(&base, Some(&overlay), 0)),
            vec![("B".to_string(), 30)]
        );
    }
}
