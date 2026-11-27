//! Figure checks: whether the figures a proposition states are the ones its
//! passage prints. Built on [`super::numbers`], the answer path's number
//! reader, so a figure reads here as an answer's figure does. Nothing here
//! knows a currency, language or market: a number the reader cannot read one
//! way leaves the figure `Unchecked`, never `Differs`.
//!
//! Each material figure (an amount, a count with a scale word, a rate) is
//! compared with every number of the first passage, and of each later one
//! while it is not found (a chunk, then its whole Source). A fact takes the
//! worst verdict among its figures; a fact with none is `Unchecked`.

use serde::{Deserialize, Serialize};

use super::model::EvidenceFigure;
use super::numbers::{self, FigureMatch, FigurePassage, Place, Unit};

/// How a fact's figures stand against its passage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigureVerdict {
    /// Equal after a scale or unit conversion the reader knows.
    Matched,
    /// Within half a unit of the stated last digit.
    Rounded,
    /// No passage number matches.
    NotFound,
    /// A passage number of the same kind and currency is the figure times a
    /// power of ten, or its negation.
    Differs,
    /// No figure, or one the reader cannot read one way.
    Unchecked,
}

impl FigureVerdict {
    /// What a published support stores: `Differs` is never published and
    /// `Unchecked` stores nothing.
    pub(crate) fn stored(self) -> Option<EvidenceFigure> {
        match self {
            Self::Matched => Some(EvidenceFigure::Matched),
            Self::Rounded => Some(EvidenceFigure::Rounded),
            Self::NotFound => Some(EvidenceFigure::NotFound),
            Self::Differs | Self::Unchecked => None,
        }
    }

    /// Worse verdicts first: a fact takes the worst of its figures.
    fn severity(self) -> u8 {
        match self {
            Self::Differs => 4,
            Self::NotFound => 3,
            Self::Unchecked => 2,
            Self::Rounded => 1,
            Self::Matched => 0,
        }
    }
}

/// One stated figure and the passage number its verdict rests on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FigureCheck {
    /// The figure as the proposition prints it.
    pub stated: String,
    pub verdict: FigureVerdict,
    /// The passage number it matched, or for `Differs` the one it differs from.
    pub cited: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FactCheck {
    pub verdict: FigureVerdict,
    pub figures: Vec<FigureCheck>,
}

fn verdict_of(found: FigureMatch) -> FigureVerdict {
    match found {
        FigureMatch::Matched => FigureVerdict::Matched,
        FigureMatch::Rounded => FigureVerdict::Rounded,
        FigureMatch::Differs => FigureVerdict::Differs,
        FigureMatch::NotFound => FigureVerdict::NotFound,
    }
}

/// One stated figure against one passage.
/// A passage a figure is read against, scanned at most once for all the
/// figures checked against it. Held as `&dyn` so a chunk and its Source,
/// borrowed for different lifetimes, are checked in one list.
pub(crate) trait Passage {
    fn text(&self) -> &str;
    fn candidates(
        &self,
        amount: Option<&numbers::Printed>,
    ) -> (Vec<numbers::Candidate>, &[(usize, usize)]);
}

impl Passage for FigurePassage<'_> {
    fn text(&self) -> &str {
        FigurePassage::text(self)
    }

    fn candidates(
        &self,
        amount: Option<&numbers::Printed>,
    ) -> (Vec<numbers::Candidate>, &[(usize, usize)]) {
        FigurePassage::candidates(self, amount)
    }
}

fn against(stated: &numbers::Stated, passage: &dyn Passage) -> (FigureVerdict, Option<String>) {
    let amount = matches!(stated.printed.unit, Unit::Amount(_)).then_some(&stated.printed);
    let (candidates, unsure) = passage.candidates(amount);
    let (found, index) = numbers::match_figure(stated, &candidates, unsure);
    let cited = index
        .and_then(|index| match candidates[index].place {
            Place::Span { start, end } => passage.text().get(start..end),
            Place::Pointer(_) => None,
        })
        .map(str::to_owned);
    let verdict = verdict_of(found);
    let unreadable = !stated.sure || !unsure.is_empty();
    if unreadable && !matches!(verdict, FigureVerdict::Matched | FigureVerdict::Rounded) {
        return (FigureVerdict::Unchecked, None);
    }
    (verdict, cited)
}

/// Checks `proposition` against `passages`, nearest first.
pub(crate) fn check(proposition: &str, passages: &[&str]) -> FactCheck {
    let passages = passages
        .iter()
        .map(|text| FigurePassage::new(text))
        .collect::<Vec<_>>();
    check_in(
        proposition,
        &passages
            .iter()
            .map(|passage| passage as &dyn Passage)
            .collect::<Vec<_>>(),
    )
}

/// [`check`] against passages a caller keeps for many propositions, so each
/// passage, a whole Source included, is scanned once.
pub(crate) fn check_in(proposition: &str, passages: &[&dyn Passage]) -> FactCheck {
    let (stated, unrecognized) = numbers::stated_figures(proposition);
    let mut figures = Vec::with_capacity(stated.len());
    for figure in &stated {
        let mut best: Option<(FigureVerdict, Option<String>)> = None;
        for passage in passages {
            let (verdict, cited) = against(figure, *passage);
            if best
                .as_ref()
                .is_none_or(|(kept, _)| verdict.severity() < kept.severity())
            {
                best = Some((verdict, cited));
            }
            if matches!(verdict, FigureVerdict::Matched | FigureVerdict::Rounded) {
                break;
            }
        }
        let (verdict, cited) = best.unwrap_or((FigureVerdict::Unchecked, None));
        figures.push(FigureCheck {
            stated: proposition[figure.start..figure.end].to_owned(),
            verdict,
            cited,
        });
    }
    let worst = figures
        .iter()
        .map(|figure| figure.verdict)
        .max_by_key(|verdict| verdict.severity());
    let verdict = match worst {
        None => FigureVerdict::Unchecked,
        // A figure the reader could not read keeps the fact from looking checked.
        Some(verdict)
            if unrecognized && verdict.severity() < FigureVerdict::Unchecked.severity() =>
        {
            FigureVerdict::Unchecked
        }
        Some(verdict) => verdict,
    };
    FactCheck { verdict, figures }
}

/// Which way a figure moved from an ended fact to the fact that ended it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FigureDirection {
    Lower,
    Higher,
    Same,
}

/// The figures of an ended fact and of the fact that ended it. `from` and
/// `to` are each proposition's figure as printed when it states exactly one.
/// `comparable` when each states exactly one, read one way, of the same kind,
/// currency and per-share nature; only then is there a `direction`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct FigureChange {
    pub from: Option<String>,
    pub to: Option<String>,
    pub comparable: bool,
    pub direction: Option<FigureDirection>,
}

pub(crate) fn figure_change(from: &str, to: &str) -> FigureChange {
    let one = |text: &str| {
        let (figures, unrecognized) = numbers::stated_figures(text);
        let figure = match figures.as_slice() {
            [figure] => Some(figure.clone()),
            _ => None,
        };
        let printed = figure
            .as_ref()
            .map(|figure| text[figure.start..figure.end].to_owned());
        let sure = figure.filter(|figure| figure.sure && !unrecognized);
        (printed, sure, numbers::per_share_label(text))
    };
    let (from_text, from_figure, from_per_share) = one(from);
    let (to_text, to_figure, to_per_share) = one(to);
    let direction = match (from_figure, to_figure) {
        (Some(old), Some(new)) if from_per_share == to_per_share => {
            let (old, new) = (&old.printed, &new.printed);
            let kind = match (old.unit, new.unit) {
                (Unit::Amount(_), Unit::Amount(_)) => {
                    old.currency.is_some() == new.currency.is_some()
                }
                (old, new) => old == new,
            };
            let currency = numbers::currencies_agree(old.currency, new.currency);
            (kind && currency).then(|| {
                let quantity = |printed: &numbers::Printed| match printed.unit {
                    Unit::Amount(scale) => (printed.value * scale, printed.precision * scale),
                    _ => (printed.value, printed.precision),
                };
                let ((old, old_precision), (new, new_precision)) = (quantity(old), quantity(new));
                if (new - old).abs() <= old_precision.min(new_precision) {
                    FigureDirection::Same
                } else if new < old {
                    FigureDirection::Lower
                } else {
                    FigureDirection::Higher
                }
            })
        }
        _ => None,
    };
    FigureChange {
        from: from_text,
        to: to_text,
        comparable: direction.is_some(),
        direction,
    }
}

#[cfg(test)]
mod tests;
