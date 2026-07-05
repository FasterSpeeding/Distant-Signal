use serde_repr::{Deserialize_repr, Serialize_repr};

/// Status severity scale. Mirrors TfL's `statusSeverity` codes 0–14 where the
/// meanings carry over, with NR-specific extensions above 14. Lower is worse,
/// except 0 (Special Service) and 10 (Good Service) which are canonical "fine"
/// states. Sort ascending for disrupted-lines-first UI ordering.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum Severity {
    SpecialService = 0,
    Closed = 1,
    Suspended = 2,
    PartSuspended = 3,
    PlannedClosure = 4,
    PartClosure = 5,
    SevereDelays = 6,
    ReducedService = 7,
    /// Rail replacement bus service.
    BusService = 8,
    MinorDelays = 9,
    GoodService = 10,
    PartClosed = 11,
    ExitOnly = 12,
    NoStepFree = 13,
    ChangeOfFrequency = 14,
    /// Post-incident catch-up (NR extension).
    Recovering = 20,
    /// Services running on an alternative route (NR extension).
    Diverted = 21,
}

impl Severity {
    pub fn description(self) -> &'static str {
        match self {
            Self::SpecialService => "Special Service",
            Self::Closed => "Closed",
            Self::Suspended => "Suspended",
            Self::PartSuspended => "Part Suspended",
            Self::PlannedClosure => "Planned Closure",
            Self::PartClosure => "Part Closure",
            Self::SevereDelays => "Severe Delays",
            Self::ReducedService => "Reduced Service",
            Self::BusService => "Rail Replacement",
            Self::MinorDelays => "Minor Delays",
            Self::GoodService => "Good Service",
            Self::PartClosed => "Part Closed",
            Self::ExitOnly => "Exit Only",
            Self::NoStepFree => "No Step Free Access",
            Self::ChangeOfFrequency => "Change of Frequency",
            Self::Recovering => "Recovering",
            Self::Diverted => "Diverted",
        }
    }
}
