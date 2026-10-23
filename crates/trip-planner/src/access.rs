//! Access and egress legs for a journey that starts or ends somewhere with
//! no train of its own (DS-135: a Tube, DLR or tram stop such as Uxbridge).
//!
//! The end becomes a virtual stop: one TIPLOC, under a CRS-like code of its
//! own, joined to each candidate National Rail station by a fixed link of
//! its own (a Tube ride or a walk). Every search in this crate already
//! seeds fixed links from its origins and follows them into its
//! destinations, so a virtual origin is a multi-source search (every
//! candidate at the departure time plus its access minutes) and a virtual
//! destination a multi-target one (every candidate plus its egress
//! minutes), with nothing else changed: Connection Scan, RAPTOR, the
//! arrive-by search, waypoints and vias all work unmodified.
//!
//! [`with_access`] adds the virtual stops to a COPY of the interchange
//! data, for one request only: the cached graph is never changed. The
//! links run one way only (out of a virtual origin, into a virtual
//! destination), so no journey can pass through a virtual stop on its way
//! somewhere else.

use schedule_query::{FixedLink, InterchangeData};

/// One access (or egress) link between a virtual stop and a station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessLink {
    /// The station's CRS.
    pub crs: String,
    /// `TUBE`, `METRO` or `WALK`, as a CIF fixed link names its mode.
    pub mode: String,
    /// Strictly positive.
    pub minutes: i32,
    /// The clock windows (`"HHMM"`, inclusive) the link can be started in,
    /// every day; empty means all day. A window past midnight is given as
    /// two (`0530`-`2359` and `0000`-`0030`).
    pub windows: Vec<(String, String)>,
}

/// One virtual end of a journey: its code (used as both its CRS and its
/// only TIPLOC) and its links.
#[derive(Debug, Clone, Copy)]
pub struct AccessEnd<'a> {
    pub code: &'a str,
    pub links: &'a [AccessLink],
}

const ALL_DAYS: &str = "1111111";

fn fixed_links(to_crs: &str, link: &AccessLink) -> Vec<FixedLink> {
    let windows = if link.windows.is_empty() {
        vec![("0000".to_owned(), "2359".to_owned())]
    } else {
        link.windows.clone()
    };
    windows
        .into_iter()
        .map(|(valid_from, valid_to)| FixedLink {
            mode: link.mode.clone(),
            to_crs: to_crs.to_owned(),
            minutes: link.minutes,
            valid_from,
            valid_to,
            days_mask: ALL_DAYS.to_owned(),
        })
        .collect()
}

fn add_stop(data: &mut InterchangeData, code: &str) {
    data.tiploc_to_crs.insert(code.to_owned(), code.to_owned());
    data.crs_to_tiplocs
        .insert(code.to_owned(), vec![code.to_owned()]);
}

/// A copy of `base` with `origin` and `destination` added as virtual stops:
/// links from the origin to each of its stations, from each of the
/// destination's stations to it, and `direct` (when given: the same link
/// shape, its `crs` ignored) from the origin straight to the destination,
/// for two stops on one line. A link to a station `base` does not know is
/// dropped. Each CRS's links stay in lookup order
/// ([`InterchangeData::sort_fixed_links`]).
pub fn with_access(
    base: &InterchangeData,
    origin: Option<AccessEnd<'_>>,
    destination: Option<AccessEnd<'_>>,
    direct: Option<&AccessLink>,
) -> InterchangeData {
    let mut data = base.clone();
    let known = |crs: &str| base.crs_to_tiplocs.contains_key(crs);
    if let Some(end) = origin {
        add_stop(&mut data, end.code);
        let links: Vec<FixedLink> = end
            .links
            .iter()
            .filter(|link| known(&link.crs))
            .flat_map(|link| fixed_links(&link.crs, link))
            .collect();
        data.fixed_links_from_crs
            .entry(end.code.to_owned())
            .or_default()
            .extend(links);
    }
    if let Some(end) = destination {
        add_stop(&mut data, end.code);
        for link in end.links.iter().filter(|link| known(&link.crs)) {
            data.fixed_links_from_crs
                .entry(link.crs.clone())
                .or_default()
                .extend(fixed_links(end.code, link));
        }
    }
    if let (Some(origin), Some(destination), Some(link)) = (origin, destination, direct) {
        data.fixed_links_from_crs
            .entry(origin.code.to_owned())
            .or_default()
            .extend(fixed_links(destination.code, link));
    }
    data.sort_fixed_links();
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn base() -> InterchangeData {
        InterchangeData {
            modal_change: schedule_query::ModalChangeBuffer::default(),
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::from([("HROW".to_owned(), "HOH".to_owned())]),
            crs_to_tiplocs: HashMap::from([("HOH".to_owned(), vec!["HROW".to_owned()])]),
            fixed_links_from_crs: HashMap::new(),
        }
    }

    fn link(crs: &str, minutes: i32) -> AccessLink {
        AccessLink {
            crs: crs.to_owned(),
            mode: "TUBE".to_owned(),
            minutes,
            windows: vec![
                ("0530".to_owned(), "2359".to_owned()),
                ("0000".to_owned(), "0030".to_owned()),
            ],
        }
    }

    #[test]
    fn links_run_out_of_the_origin_and_into_the_destination_only() {
        let links = [link("HOH", 30), link("ZZZ", 5)];
        let base = base();
        let data = with_access(
            &base,
            Some(AccessEnd {
                code: "940GA",
                links: &links,
            }),
            Some(AccessEnd {
                code: "940GB",
                links: &links,
            }),
            None,
        );
        assert_eq!(data.crs_to_tiplocs["940GA"], ["940GA"]);
        assert_eq!(data.tiploc_to_crs["940GB"], "940GB");
        // Two windows each; the unknown station is dropped.
        let out: Vec<&str> = data.fixed_links_from_crs["940GA"]
            .iter()
            .map(|l| l.to_crs.as_str())
            .collect();
        assert_eq!(out, ["HOH", "HOH"]);
        let into: Vec<&str> = data.fixed_links_from_crs["HOH"]
            .iter()
            .map(|l| l.to_crs.as_str())
            .collect();
        assert_eq!(into, ["940GB", "940GB"]);
        assert!(!data.fixed_links_from_crs.contains_key("940GB"));
        // The base is untouched.
        assert!(base.fixed_links_from_crs.is_empty());
        assert!(!base.crs_to_tiplocs.contains_key("940GA"));
    }
}
