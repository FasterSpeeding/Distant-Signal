//! OR choices of stations for `/Trips/plan` (2026-10-07): one `via` entry,
//! or one avoid-list entry, standing for ANY of several stations.
//!
//! An entry is one or more alternatives separated by `|`; each alternative
//! is a station CRS, a bus stop's or ferry terminal's `tiploc:` code, or a
//! named group `group:NAME` from the checked-in
//! `reference-data/station-groups.csv` (e.g. `group:LON`, the eighteen
//! London Terminals). `via=KGX|EUS|group:LON` is one via satisfied by
//! passing any of them. Avoid lists take groups too (`avoid=group:LON`
//! avoids every member: an avoid list already means "none of these"), but
//! not `|`. See
//! docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md.

use std::collections::BTreeMap;
use std::sync::LazyLock;

/// `reference-data/station-groups.csv`, compiled in.
const STATION_GROUPS_CSV: &str = include_str!("../../../../reference-data/station-groups.csv");

/// The prefix naming a group in a request (any case on the wire).
pub const GROUP_PREFIX: &str = "group:";

/// Group name -> member CRS codes, in file order.
static GROUPS: LazyLock<BTreeMap<String, Vec<String>>> =
    LazyLock::new(|| parse_groups(STATION_GROUPS_CSV));

/// `group,crs,name` rows. Comment (`#`) and blank lines and the header are
/// skipped, and so is a malformed row (the checked-in file's own test
/// proves it has none). A member listed twice is kept once.
fn parse_groups(csv: &str) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in csv.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with("group,") {
            continue;
        }
        let mut fields = line.splitn(3, ',');
        let (Some(group), Some(crs)) = (fields.next(), fields.next()) else {
            continue;
        };
        let (group, crs) = (group.trim(), crs.trim());
        let valid = (2..=12).contains(&group.len())
            && group
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && crs.len() == 3
            && crs.chars().all(|c| c.is_ascii_uppercase());
        if !valid {
            continue;
        }
        let members = groups.entry(group.to_string()).or_default();
        if !members.iter().any(|m| m == crs) {
            members.push(crs.to_string());
        }
    }
    groups
}

/// Every group, name -> members.
pub fn groups() -> &'static BTreeMap<String, Vec<String>> {
    &GROUPS
}

/// One request entry, parsed: see the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationChoice {
    /// The entry as applied (and echoed): its alternatives normalised and
    /// joined by `|`, a group as `group:NAME`. A single station is just its
    /// code, as before groups existed.
    pub label: String,
    /// Every station code it stands for, in order, each once: named
    /// stations as normalised codes, a group as its members.
    pub codes: Vec<String>,
    /// The groups it names (without the prefix), in order.
    pub groups: Vec<String>,
}

impl StationChoice {
    /// One plain station, no `|` and no group: the pre-2026-10-07 shape,
    /// whose checks (e.g. a via equal to the origin is a 400) still apply.
    pub fn is_single_station(&self) -> bool {
        self.groups.is_empty() && self.codes.len() == 1
    }
}

/// Parses one entry of the list `list` (its wire name, for messages).
/// `alternatives`: whether `|` is allowed (`via`), else a 400 message (the
/// avoid lists). `Err` is the message of a 400.
pub fn parse_choice(list: &str, raw: &str, alternatives: bool) -> Result<StationChoice, String> {
    if !alternatives && raw.contains('|') {
        return Err(format!(
            "{list}: '{}' uses '|', which only via takes; {list} already applies to every \
             station it lists, so separate them with commas",
            raw.trim()
        ));
    }
    let mut choice = StationChoice {
        label: String::new(),
        codes: Vec::new(),
        groups: Vec::new(),
    };
    let mut labels: Vec<String> = Vec::new();
    for part in raw.split('|').map(str::trim) {
        if part.is_empty() {
            return Err(format!("{list}: '{}' has an empty alternative", raw.trim()));
        }
        let is_group = part
            .get(..GROUP_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(GROUP_PREFIX));
        if is_group {
            let name = part[GROUP_PREFIX.len()..].trim().to_ascii_uppercase();
            let Some(members) = groups().get(&name) else {
                let known: Vec<String> = groups()
                    .keys()
                    .map(|g| format!("{GROUP_PREFIX}{g}"))
                    .collect();
                return Err(format!(
                    "{list}: '{part}' is not a known station group (known: {})",
                    known.join(", ")
                ));
            };
            push_once(&mut labels, format!("{GROUP_PREFIX}{name}"));
            push_once(&mut choice.groups, name);
            for member in members {
                push_once(&mut choice.codes, member.clone());
            }
        } else {
            let code = common::location_naming::normalize_location_code(part);
            push_once(&mut labels, code.clone());
            push_once(&mut choice.codes, code);
        }
    }
    choice.label = labels.join("|");
    Ok(choice)
}

fn push_once(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checked_in_groups_parse_completely() {
        let rows = STATION_GROUPS_CSV
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("group,"))
            .count();
        let parsed: usize = groups().values().map(Vec::len).sum();
        assert_eq!(parsed, rows, "every row is valid and no member repeats");
        let london = &groups()["LON"];
        assert_eq!(london.len(), 18);
        for crs in [
            "KGX", "STP", "EUS", "PAD", "WAT", "VIC", "LBG", "WAE", "VXH",
        ] {
            assert!(london.iter().any(|m| m == crs), "{crs}");
        }
    }

    #[test]
    fn malformed_rows_are_skipped() {
        let parsed =
            parse_groups("group,crs,name\nX,ABC\nAB,abc,x\nAB,ABCD\nAB,KGX,\nAB,KGX,again\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed["AB"], vec!["KGX".to_string()]);
    }

    #[test]
    fn a_choice_names_stations_alternatives_and_groups() {
        let single = parse_choice("via", " kgx ", true).unwrap();
        assert_eq!(single.label, "KGX");
        assert_eq!(single.codes, vec!["KGX".to_string()]);
        assert!(single.is_single_station());

        let alternatives = parse_choice("via", "kgx | eus|KGX|tiploc:sanwbus", true).unwrap();
        assert_eq!(alternatives.label, "KGX|EUS|tiploc:SANWBUS");
        assert_eq!(alternatives.codes, vec!["KGX", "EUS", "tiploc:SANWBUS"]);
        assert!(!alternatives.is_single_station());

        let group = parse_choice("via", "Group:lon", true).unwrap();
        assert_eq!(group.label, "group:LON");
        assert_eq!(group.codes.len(), 18);
        assert_eq!(group.groups, vec!["LON".to_string()]);
        assert!(!group.is_single_station());

        let mixed = parse_choice("via", "CBG|group:LON|KGX", true).unwrap();
        assert_eq!(mixed.codes.len(), 19, "KGX counted once");
        assert_eq!(mixed.codes[0], "CBG");
    }

    #[test]
    fn bad_choices_are_explained() {
        let err = parse_choice("via", "group:XYZ", true).unwrap_err();
        assert!(
            err.contains("not a known station group") && err.contains("group:LON"),
            "{err}"
        );
        let err = parse_choice("via", "KGX||EUS", true).unwrap_err();
        assert!(err.contains("empty alternative"), "{err}");
        let err = parse_choice("avoid", "KGX|EUS", false).unwrap_err();
        assert!(err.contains("only via") && err.contains("commas"), "{err}");
        // A group in an avoid list is fine.
        assert_eq!(
            parse_choice("avoid", "group:LON", false)
                .unwrap()
                .codes
                .len(),
            18
        );
    }
}
