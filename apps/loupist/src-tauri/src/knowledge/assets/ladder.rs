//! The Loupist's four-step read on a thesis and the rules for moving it.
//! Pure: callers bring the current state and the evidence dates.
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::asset_error;
use crate::knowledge::KnowledgeResult;

/// "Holding up" is stored as `holding`; the label never reads as ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Holding,
    Weakening,
    Challenged,
    Broken,
}

impl Verdict {
    pub fn step(self) -> u8 {
        match self {
            Self::Holding => 1,
            Self::Weakening => 2,
            Self::Challenged => 3,
            Self::Broken => 4,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Holding => "holding",
            Self::Weakening => "weakening",
            Self::Challenged => "challenged",
            Self::Broken => "broken",
        }
    }

    /// English label for model-facing text and bounded growth summaries.
    pub fn label(self) -> &'static str {
        match self {
            Self::Holding => "Holding up",
            Self::Weakening => "Weakening",
            Self::Challenged => "Challenged",
            Self::Broken => "Broken",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
    }
}

/// A move is one step up or down the ladder; a crossed line is the only way to
/// jump, and it always lands on Broken.
pub fn validate_step(current: Verdict, to: Verdict) -> KnowledgeResult<()> {
    let delta = i16::from(to.step()) - i16::from(current.step());
    match delta {
        1 | -1 => Ok(()),
        0 => Err(asset_error(
            "thesis_same_step",
            "thesis_update.to must differ from the current read",
        )),
        _ => Err(asset_error(
            "thesis_two_steps",
            "thesis_update.to must be one step from the current read (Holding up, Weakening, Challenged, Broken)",
        )),
    }
}

/// Whether a reading crosses the user's line. Strict, like Journal reviews.
pub fn crossed(direction: &str, threshold: f64, observed: f64) -> bool {
    if direction == "below" {
        observed < threshold
    } else {
        observed > threshold
    }
}

/// The user undid a Loupist move. Moving the same way again needs a fact dated
/// after the undone move's fact; the opposite direction is unconstrained.
pub fn validate_after_undo(
    undone: Option<(Verdict, Verdict, NaiveDate)>,
    current: Verdict,
    to: Verdict,
    evidence_date: NaiveDate,
) -> KnowledgeResult<()> {
    let Some((from, undone_to, fact_date)) = undone else {
        return Ok(());
    };
    let same_direction = (undone_to.step() > from.step()) == (to.step() > current.step());
    if same_direction && evidence_date <= fact_date {
        return Err(asset_error(
            "thesis_after_undo",
            "the user undid this move; moving the read the same way again needs evidence dated after the undone move's fact date",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_move_is_exactly_one_step() {
        assert!(validate_step(Verdict::Holding, Verdict::Weakening).is_ok());
        assert!(validate_step(Verdict::Challenged, Verdict::Weakening).is_ok());
        assert_eq!(
            validate_step(Verdict::Holding, Verdict::Challenged)
                .unwrap_err()
                .code(),
            "thesis_two_steps"
        );
        assert_eq!(
            validate_step(Verdict::Broken, Verdict::Holding)
                .unwrap_err()
                .code(),
            "thesis_two_steps"
        );
        assert_eq!(
            validate_step(Verdict::Weakening, Verdict::Weakening)
                .unwrap_err()
                .code(),
            "thesis_same_step"
        );
    }

    #[test]
    fn a_line_is_crossed_strictly_in_its_direction() {
        assert!(crossed("above", 25.0, 25.1));
        assert!(!crossed("above", 25.0, 25.0));
        assert!(crossed("below", 16.0, 14.8));
        assert!(!crossed("below", 16.0, 16.0));
    }

    #[test]
    fn after_an_undo_the_same_direction_needs_a_later_fact() {
        let undone = Some((Verdict::Holding, Verdict::Weakening, day("2026-10-30")));
        let refused = validate_after_undo(
            undone,
            Verdict::Holding,
            Verdict::Weakening,
            day("2026-10-30"),
        )
        .unwrap_err();
        assert_eq!(refused.code(), "thesis_after_undo");
        assert!(validate_after_undo(
            undone,
            Verdict::Holding,
            Verdict::Weakening,
            day("2026-10-31")
        )
        .is_ok());
        // An upgrade is not the undone direction.
        assert!(validate_after_undo(
            Some((Verdict::Weakening, Verdict::Challenged, day("2026-10-30"))),
            Verdict::Weakening,
            Verdict::Holding,
            day("2026-10-01")
        )
        .is_ok());
    }
}
