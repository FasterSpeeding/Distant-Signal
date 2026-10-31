//! Operator filters' vocabulary for `/Trips/plan` (planner operator
//! filtering spec 2026-10-31, §4.2 and §6.4): named groups of operators
//! (`group:GTR`), from `reference-data/operator-groups.csv`; display names
//! for the timetable codes `tocs` does not name, from
//! `reference-data/operator-names.csv`; and the parsing of a request's
//! `operator` / `avoidOperator` list into operator ids.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use schedule_query::{DsOperator, OperatorId, OperatorMask};

/// `reference-data/operator-groups.csv`, compiled in.
const OPERATOR_GROUPS_CSV: &str = include_str!("../../../../reference-data/operator-groups.csv");
/// `reference-data/operator-names.csv`, compiled in.
const OPERATOR_NAMES_CSV: &str = include_str!("../../../../reference-data/operator-names.csv");

/// The prefix naming a group in a request (any case on the wire).
pub const GROUP_PREFIX: &str = "group:";

/// Entries one operator list may hold (the avoid lists' cap).
pub const MAX_OPERATOR_ENTRIES: usize = 8;

/// One named group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorGroup {
    pub name: String,
    pub members: Vec<OperatorId>,
}

static GROUPS: LazyLock<BTreeMap<String, OperatorGroup>> =
    LazyLock::new(|| parse_groups(OPERATOR_GROUPS_CSV));

static NAMES: LazyLock<BTreeMap<String, String>> =
    LazyLock::new(|| parse_names(OPERATOR_NAMES_CSV));

fn data_lines<'a>(csv: &'a str, header: &'a str) -> impl Iterator<Item = &'a str> {
    csv.lines()
        .map(str::trim)
        .filter(move |line| !line.is_empty() && !line.starts_with('#') && *line != header)
}

/// `group,member,name` rows; a malformed row is skipped (the checked-in
/// file's own test proves it has none).
fn parse_groups(csv: &str) -> BTreeMap<String, OperatorGroup> {
    let mut groups: BTreeMap<String, OperatorGroup> = BTreeMap::new();
    for line in data_lines(csv, "group,member,name") {
        let mut fields = line.splitn(3, ',');
        let (Some(group), Some(member)) = (fields.next(), fields.next()) else {
            continue;
        };
        let name = fields.next().unwrap_or_default().trim();
        let group = group.trim();
        let valid = (2..=12).contains(&group.len())
            && group
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        let Some(member) = OperatorId::from_code(member.trim()).filter(|_| valid) else {
            continue;
        };
        let entry = groups
            .entry(group.to_owned())
            .or_insert_with(|| OperatorGroup {
                name: group.to_owned(),
                members: Vec::new(),
            });
        if !name.is_empty() {
            name.clone_into(&mut entry.name);
        }
        if !entry.members.contains(&member) {
            entry.members.push(member);
        }
    }
    groups
}

fn parse_names(csv: &str) -> BTreeMap<String, String> {
    data_lines(csv, "code,name")
        .filter_map(|line| {
            let (code, name) = line.split_once(',')?;
            let id = OperatorId::from_atoc(code)?;
            Some((id.code()?, name.trim().to_owned()))
        })
        .collect()
}

/// Every group, by code.
pub fn groups() -> &'static BTreeMap<String, OperatorGroup> {
    &GROUPS
}

/// DS display names for timetable codes `tocs` doesn't name (`TW`).
pub fn names() -> &'static BTreeMap<String, String> {
    &NAMES
}

/// `GET /Trips/operator-groups`'s body: the groups (members as API codes),
/// the DS operators and the extra names.
pub fn catalogue() -> serde_json::Value {
    serde_json::json!({
        "groups": groups()
            .iter()
            .map(|(code, group)| serde_json::json!({
                "code": code,
                "name": group.name,
                "members": group.members.iter().filter_map(|id| id.code()).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "dsOperators": DsOperator::ALL
            .iter()
            .map(|op| serde_json::json!({ "code": op.code(), "name": op.name() }))
            .collect::<Vec<_>>(),
        "names": names(),
    })
}

/// Every ATOC code the published timetable runs from today on, with its
/// `tocs` name, else its DS name ([`names`]), else none: what the planner's
/// operator picker offers (a `tocs` code with no train in the window, such
/// as `ES`, would match nothing). Sorted by code. Backs
/// `GET /Trips/operators`.
pub async fn running_operators(
    pool: &sqlx::PgPool,
) -> anyhow::Result<Vec<(String, Option<String>)>> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT s.code, t.name FROM \
           (SELECT DISTINCT operator_atoc AS code FROM schedule_services \
             WHERE service_date >= current_date AND operator_atoc IS NOT NULL) s \
         LEFT JOIN tocs t ON t.atoc_code::text = s.code \
         ORDER BY s.code",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(code, name)| {
            let name = name.or_else(|| names().get(&code).cloned());
            (code, name)
        })
        .collect())
}

/// `GET /Trips/operators`'s body: [`running_operators`] as `operators`,
/// with [`catalogue`]'s groups and `ds:` operators.
pub fn planner_catalogue(running: &[(String, Option<String>)]) -> serde_json::Value {
    let mut body = catalogue();
    body["operators"] = running
        .iter()
        .map(|(code, name)| serde_json::json!({ "code": code, "name": name }))
        .collect();
    if let Some(object) = body.as_object_mut() {
        object.remove("names");
    }
    body
}

/// A request's operator list, parsed: the entries as given (normalised),
/// the operators they expand to (deduplicated, in order), and each group
/// named -> its members' codes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperatorList {
    pub entries: Vec<String>,
    pub operators: Vec<OperatorId>,
    pub groups: BTreeMap<String, Vec<String>>,
}

impl OperatorList {
    /// The expanded operators' API codes.
    pub fn codes(&self) -> Vec<String> {
        self.operators.iter().filter_map(|id| id.code()).collect()
    }

    /// The ATOC codes named directly, not through a group: what must be
    /// checked against `tocs` and the timetable (a group's members are DS's
    /// own data, and one may run no train in a given week).
    pub fn named_atoc_codes(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| !entry.starts_with(GROUP_PREFIX) && !entry.starts_with("ds:"))
            .cloned()
            .collect()
    }

    pub fn mask(&self) -> OperatorMask {
        OperatorMask::of(self.operators.iter().copied())
    }
}

/// Parses `name=` (`operator` or `avoidOperator`): comma-separated ATOC
/// codes, `ds:` operators and `group:NAME`s, trimmed and case-insensitive;
/// empty entries dropped. `Ok(None)` when nothing is left. `Err` is a 400's
/// text: a malformed entry, an unknown group or `ds:` name, or more than
/// [`MAX_OPERATOR_ENTRIES`] entries. Unknown ATOC codes are checked by the
/// caller, against `tocs` and the timetable.
pub fn parse_operator_list(name: &str, raw: Option<&str>) -> Result<Option<OperatorList>, String> {
    let mut list = OperatorList::default();
    for entry in raw.unwrap_or_default().split(',').map(str::trim) {
        if entry.is_empty() {
            continue;
        }
        let lower = entry.to_ascii_lowercase();
        let (normalised, members): (String, Vec<OperatorId>) =
            if let Some(group) = lower.strip_prefix(GROUP_PREFIX) {
                let group = group.to_ascii_uppercase();
                let Some(found) = groups().get(&group) else {
                    return Err(format!("{name}: unknown operator group group:{group}"));
                };
                list.groups.insert(
                    group.clone(),
                    found.members.iter().filter_map(|id| id.code()).collect(),
                );
                (format!("{GROUP_PREFIX}{group}"), found.members.clone())
            } else if lower.starts_with("ds:") {
                let Some(op) = DsOperator::from_code(entry) else {
                    return Err(format!(
                        "{name}: unknown operator {entry} (the ds: operators are {})",
                        DsOperator::ALL
                            .iter()
                            .map(|op| op.code())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                };
                (op.code().to_owned(), vec![OperatorId::ds(op)])
            } else {
                let Some(id) = OperatorId::from_atoc(entry) else {
                    return Err(format!(
                        "{name} entries must be a 2-character ATOC code, ds:NAME or group:NAME \
                         (got {entry})"
                    ));
                };
                (id.code().unwrap_or_default(), vec![id])
            };
        if !list.entries.contains(&normalised) {
            list.entries.push(normalised);
        }
        for member in members {
            if !list.operators.contains(&member) {
                list.operators.push(member);
            }
        }
    }
    if list.entries.len() > MAX_OPERATOR_ENTRIES {
        return Err(format!(
            "{name} takes at most {MAX_OPERATOR_ENTRIES} entries"
        ));
    }
    Ok((!list.entries.is_empty()).then_some(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_rows(csv: &str, header: &str) -> usize {
        data_lines(csv, header).count()
    }

    #[test]
    fn every_checked_in_group_row_parses() {
        let members: usize = groups().values().map(|g| g.members.len()).sum();
        assert_eq!(members, data_rows(OPERATOR_GROUPS_CSV, "group,member,name"));
        let gtr = &groups()["GTR"];
        assert_eq!(
            gtr.members
                .iter()
                .filter_map(|id| id.code())
                .collect::<Vec<_>>(),
            ["TL", "GN", "SN", "GX"]
        );
        assert!(gtr.name.starts_with("GTR ("), "{}", gtr.name);
        assert!(
            groups()["TFL"]
                .members
                .contains(&OperatorId::ds(DsOperator::Dlr))
        );
    }

    #[test]
    fn every_checked_in_name_row_parses() {
        assert_eq!(names().len(), data_rows(OPERATOR_NAMES_CSV, "code,name"));
        assert_eq!(names()["TW"], "Tyne and Wear Metro");
    }

    #[test]
    fn lists_expand_groups_and_deduplicate() {
        let list = parse_operator_list("operator", Some(" vt, group:gtr ,TL,,ds:dlr"))
            .unwrap()
            .unwrap();
        assert_eq!(list.entries, ["VT", "group:GTR", "TL", "ds:DLR"]);
        assert_eq!(list.codes(), ["VT", "TL", "GN", "SN", "GX", "ds:DLR"]);
        assert_eq!(list.named_atoc_codes(), ["VT", "TL"]);
        assert_eq!(list.groups["GTR"], ["TL", "GN", "SN", "GX"]);
        assert!(list.mask().contains(OperatorId::from_atoc("GX").unwrap()));
    }

    #[test]
    fn empty_lists_are_none() {
        assert_eq!(parse_operator_list("operator", None), Ok(None));
        assert_eq!(parse_operator_list("operator", Some(" , ")), Ok(None));
    }

    #[test]
    fn bad_entries_are_400_texts() {
        for raw in ["VTX", "V", "ds:NOPE", "group:NOPE", "V-"] {
            let err = parse_operator_list("avoidOperator", Some(raw)).unwrap_err();
            assert!(err.starts_with("avoidOperator"), "{raw}: {err}");
        }
        let nine = "A1,A2,A3,A4,A5,A6,A7,A8,A9";
        assert!(parse_operator_list("operator", Some(nine)).is_err());
        assert!(parse_operator_list("operator", Some(&nine[..23])).is_ok());
    }

    #[test]
    fn the_planner_catalogue_adds_the_running_operators() {
        let body = planner_catalogue(&[
            ("TW".to_owned(), Some("Tyne and Wear Metro".to_owned())),
            ("VT".to_owned(), Some("Avanti West Coast".to_owned())),
        ]);
        assert_eq!(body["operators"][0]["code"], "TW");
        assert_eq!(body["operators"][1]["name"], "Avanti West Coast");
        assert_eq!(body["groups"][0]["code"], "GTR");
        assert_eq!(body["dsOperators"][1]["name"], "London Trams");
        assert!(body.get("names").is_none());
    }

    #[test]
    fn the_catalogue_lists_groups_ds_operators_and_names() {
        let body = catalogue();
        assert_eq!(body["groups"][0]["code"], "GTR");
        assert_eq!(body["dsOperators"][0]["code"], "ds:DLR");
        assert_eq!(body["names"]["QC"], "CalMac ferries");
    }
}
