//! Series operations: period growth, return and risk summary, moving average, sum.
use super::*;

const MAX_LISTED_PERIODS: usize = 400;
const MAX_COLUMN_CHARS: usize = 256;

pub(super) struct Series {
    pub(super) values: Vec<f64>,
    pub(super) labels: Option<Vec<String>>,
    provenance: Value,
}

impl Series {
    pub(super) fn label(&self, index: usize) -> String {
        self.labels
            .as_ref()
            .map_or_else(|| format!("#{}", index + 1), |labels| labels[index].clone())
    }

    pub(super) fn record(&self, outcome: &mut Outcome) {
        let last = self.values.len() - 1;
        outcome.inputs.push(json!({
            "name": "series",
            "count": self.values.len(),
            "first": {"label": self.label(0), "value": format::round15(self.values[0])},
            "last": {"label": self.label(last), "value": format::round15(self.values[last])},
            "provenance": self.provenance,
        }));
    }
}

/// An explicit array (with optional `dates`) or a dataset column
/// `{dataset_id, revision, rows_pointer?, column, date_column?}`. Series run
/// oldest to newest; date labels that run newest first are reversed. A dataset
/// column without calendar dates is taken in stored order, and the result
/// says that oldest first was assumed (some providers store newest first).
pub(super) fn series(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
    minimum: usize,
) -> Result<Series, CalcError> {
    let skip_missing = request.boolean("skip_missing")?;
    let source = request.field("series").ok_or_else(|| CalcError::Missing {
        field: "series".into(),
        expected:
            "an array of numbers or {dataset_id, revision, rows_pointer, column, date_column}"
                .into(),
    })?;
    let (mut values, mut labels, provenance, skipped) = match source {
        Value::Array(items) => explicit(items, request.field("dates"), skip_missing)?,
        Value::Object(source) => {
            if request.field("dates").is_some() {
                return Err(CalcError::UnknownField {
                    field: "dates".into(),
                    expected: "series.date_column for a dataset column".into(),
                });
            }
            column(source, skip_missing, datasets)?
        }
        _ => {
            return Err(CalcError::invalid(
                "series",
                "an array of numbers or a dataset column object",
            ))
        }
    };
    if skipped > 0 {
        outcome
            .notes
            .push(format!("Dropped {skipped} missing values (skip_missing)."));
    }
    if values.len() < minimum {
        return Err(CalcError::invalid(
            "series",
            format!("at least {minimum} values after missing values are removed"),
        ));
    }
    let dataset = matches!(source, Value::Object(_));
    if dataset
        && labels
            .as_deref()
            .is_none_or(|dates| matches!(date_order(dates), DateOrder::NotDates))
    {
        outcome.notes.push(
            "Rows were taken in stored order and oldest first was assumed, because no series.date_column with calendar dates was given; some providers store newest first, so pass a date column to order them."
                .into(),
        );
    }
    if let Some(dates) = labels.as_mut() {
        match date_order(dates) {
            DateOrder::Ascending | DateOrder::NotDates => {}
            DateOrder::Descending => {
                dates.reverse();
                values.reverse();
                outcome
                    .notes
                    .push("Dates ran newest first; the series was reordered oldest first.".into());
            }
            DateOrder::Unordered => {
                return Err(CalcError::invalid(
                    if matches!(source, Value::Array(_)) {
                        "dates"
                    } else {
                        "series.date_column"
                    },
                    "dates in chronological order without duplicates",
                ))
            }
        }
    }
    Ok(Series {
        values,
        labels,
        provenance,
    })
}

type Resolved = (Vec<f64>, Option<Vec<String>>, Value, usize);

fn explicit(
    items: &[Value],
    dates: Option<&Value>,
    skip_missing: bool,
) -> Result<Resolved, CalcError> {
    if items.len() > MAX_SERIES {
        return Err(CalcError::invalid(
            "series",
            format!("at most {MAX_SERIES} values"),
        ));
    }
    let dates = dates
        .map(|dates| {
            let dates = dates
                .as_array()
                .filter(|dates| dates.len() == items.len())
                .ok_or_else(|| {
                    CalcError::invalid("dates", "an array of date labels, one per series value")
                })?;
            dates
                .iter()
                .enumerate()
                .map(|(index, date)| bounded_text(date, &format!("dates[{index}]")))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let mut values = Vec::with_capacity(items.len());
    let mut labels = dates.as_ref().map(|_| Vec::with_capacity(items.len()));
    let mut skipped = 0;
    for (index, item) in items.iter().enumerate() {
        if item.is_null() {
            if skip_missing {
                skipped += 1;
                continue;
            }
            return Err(CalcError::invalid(
                &format!("series[{index}]"),
                "a number; a missing value is not zero, so remove it or set skip_missing",
            ));
        }
        values.push(plain_number(item, &format!("series[{index}]"))?);
        if let (Some(labels), Some(dates)) = (labels.as_mut(), dates.as_ref()) {
            labels.push(dates[index].clone());
        }
    }
    Ok((values, labels, json!({"kind": "provided"}), skipped))
}

fn column(
    source: &Map<String, Value>,
    skip_missing: bool,
    datasets: &mut dyn DatasetSource,
) -> Result<Resolved, CalcError> {
    for key in source.keys() {
        if !matches!(
            key.as_str(),
            "dataset_id" | "revision" | "rows_pointer" | "column" | "date_column"
        ) {
            return Err(CalcError::UnknownField {
                field: format!("series.{key}"),
                expected: "dataset_id, revision, rows_pointer, column, date_column".into(),
            });
        }
    }
    let name = |field: &str| -> Result<Option<String>, CalcError> {
        source
            .get(field)
            .map(|value| {
                value
                    .as_str()
                    .filter(|name| !name.is_empty() && name.chars().count() <= MAX_COLUMN_CHARS)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        CalcError::invalid(&format!("series.{field}"), "a column name in each row")
                    })
            })
            .transpose()
    };
    let column = name("column")?.ok_or_else(|| CalcError::Missing {
        field: "series.column".into(),
        expected: "the row field holding the numbers".into(),
    })?;
    let date_column = name("date_column")?;
    let (reference, payload) = dataset(source, "series", datasets)?;
    let rows_pointer = pointer(source.get("rows_pointer"), "series.rows_pointer", false)?;
    let rows = payload
        .pointer(&rows_pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| CalcError::Dataset {
            field: "series.rows_pointer".into(),
            reason: "does not select a row array in the dataset revision".into(),
        })?;
    if rows.len() > MAX_SERIES {
        return Err(CalcError::invalid(
            "series.rows_pointer",
            format!("a row array of at most {MAX_SERIES} rows"),
        ));
    }
    let mut values = Vec::with_capacity(rows.len());
    let mut labels = date_column.as_ref().map(|_| Vec::with_capacity(rows.len()));
    let mut skipped = 0;
    for (index, row) in rows.iter().enumerate() {
        let field = format!("series.column ({column}) in row {index}");
        let row = row.as_object().ok_or_else(|| CalcError::Dataset {
            field: format!("series row {index}"),
            reason: "rows must be objects with named columns".into(),
        })?;
        let cell = row.get(&column).filter(|cell| !cell.is_null());
        let Some(cell) = cell else {
            if skip_missing {
                skipped += 1;
                continue;
            }
            return Err(CalcError::Dataset {
                field,
                reason: "value is missing; a missing value is not zero, so set skip_missing to drop such rows"
                    .into(),
            });
        };
        values.push(plain_number(cell, &field)?);
        if let (Some(labels), Some(date_column)) = (labels.as_mut(), date_column.as_ref()) {
            let label = match row.get(date_column) {
                Some(Value::String(text)) if !text.is_empty() && text.len() <= 64 => text.clone(),
                Some(Value::Number(number)) => number.to_string(),
                _ => {
                    return Err(CalcError::Dataset {
                        field: format!("series.date_column ({date_column}) in row {index}"),
                        reason: "expected a date string".into(),
                    })
                }
            };
            labels.push(label);
        }
    }
    let mut provenance = json!({
        "kind": "dataset",
        "dataset_id": reference.dataset_id,
        "revision": reference.revision,
        "digest": reference.digest,
        "rows_pointer": rows_pointer,
        "column": column,
        "rows": rows.len(),
    });
    if let Some(date_column) = date_column {
        provenance["date_column"] = json!(date_column);
    }
    Ok((values, labels, provenance, skipped))
}

enum DateOrder {
    Ascending,
    Descending,
    Unordered,
    NotDates,
}

/// `YYYY`, `YYYY-MM`, `YYYY-MM-DD` or compact `YYYYMMDD`, optionally followed by a time.
pub(super) fn date_key(label: &str) -> Option<String> {
    let (date, time) = label
        .split_once(['T', ' '])
        .map_or((label, ""), |(date, time)| (date, time));
    let compact = date.replace('-', "");
    let parts = date.split('-').map(str::len).collect::<Vec<_>>();
    let shaped = matches!(parts.as_slice(), [4] | [4, 2] | [4, 2, 2] | [8] | [6]);
    (shaped && compact.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| format!("{compact}{time}"))
}

fn date_order(labels: &[String]) -> DateOrder {
    let Some(keys) = labels
        .iter()
        .map(|label| date_key(label))
        .collect::<Option<Vec<_>>>()
    else {
        return DateOrder::NotDates;
    };
    if keys.windows(2).any(|pair| pair[0].len() != pair[1].len()) {
        return DateOrder::NotDates;
    }
    if keys.windows(2).all(|pair| pair[0] < pair[1]) {
        DateOrder::Ascending
    } else if keys.windows(2).all(|pair| pair[0] > pair[1]) {
        DateOrder::Descending
    } else {
        DateOrder::Unordered
    }
}

pub(super) fn growth(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let series = series(request, datasets, outcome, 2)?;
    let count = series.values.len();
    if count - 1 > MAX_LISTED_PERIODS {
        return Err(CalcError::invalid(
            "series",
            format!(
                "at most {} values for growth_series; use returns_summary for long price series",
                MAX_LISTED_PERIODS + 1
            ),
        ));
    }
    let mut periods = Vec::with_capacity(count - 1);
    let mut undefined = Vec::new();
    for index in 1..count {
        let (previous, value) = (series.values[index - 1], series.values[index]);
        let growth = (previous > 0.0).then(|| (value / previous - 1.0) * 100.0);
        if growth.is_none() {
            undefined.push(series.label(index));
        }
        periods.push(json!({
            "label": series.label(index),
            "value": format::round15(value),
            "growth_pct": growth.map(format::round15),
            "display": growth.map(format::percent),
        }));
    }
    if !undefined.is_empty() {
        outcome.notes.push(format!(
            "Growth is null where the prior value is zero or negative: {}.",
            undefined.join(", ")
        ));
    }
    let (first, last) = (series.values[0], series.values[count - 1]);
    let (first_label, last_label) = (series.label(0), series.label(count - 1));
    outcome
        .formulas
        .push("growth = value / prior value − 1".into());
    if first > 0.0 {
        let total = (last / first - 1.0) * 100.0;
        outcome.value("total_change", total, "%")?;
        outcome.formulas.push(format!(
            "{last_label} / {first_label} − 1 = {} / {} − 1 = {}",
            format::amount(last),
            format::amount(first),
            format::percent(total)
        ));
        if last >= 0.0 {
            let periods = (count - 1) as f64;
            let compound = ((last / first).powf(1.0 / periods) - 1.0) * 100.0;
            outcome.value("compound_growth_per_period", compound, "%")?;
            outcome.formulas.push(format!(
                "({last_label} / {first_label})^(1 / {}) − 1 = {}",
                count - 1,
                format::percent(compound)
            ));
        }
    } else {
        outcome
            .notes
            .push("Total change is undefined because the first value is not positive.".into());
    }
    outcome.extra.insert("periods".into(), json!(periods));
    series.record(outcome);
    Ok(())
}

pub(super) fn returns(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let prices = match request.field("series_kind") {
        None => true,
        Some(value) => match value.as_str() {
            Some("prices") => true,
            Some("returns_pct") => false,
            _ => return Err(CalcError::invalid("series_kind", "prices or returns_pct")),
        },
    };
    let per_year = request.integer("periods_per_year", 1, 366, 252)?;
    let series = series(request, datasets, outcome, if prices { 2 } else { 1 })?;
    // Wealth index and its labels; returns-input wealth starts at 1 before the first period.
    let (wealth, labels, returns) = if prices {
        if let Some(index) = series.values.iter().position(|price| *price <= 0.0) {
            return Err(CalcError::domain(
                "series",
                format!(
                    "prices must be positive; {} is {}",
                    series.label(index),
                    format::amount(series.values[index])
                ),
            ));
        }
        let returns = series
            .values
            .windows(2)
            .map(|pair| pair[1] / pair[0] - 1.0)
            .collect::<Vec<_>>();
        let labels = (0..series.values.len())
            .map(|index| series.label(index))
            .collect::<Vec<_>>();
        (series.values.clone(), labels, returns)
    } else {
        if let Some(index) = series.values.iter().position(|value| *value < -100.0) {
            return Err(CalcError::domain(
                "series",
                format!(
                    "a period return cannot be below -100%; {} is {}",
                    series.label(index),
                    format::percent(series.values[index])
                ),
            ));
        }
        let returns = series
            .values
            .iter()
            .map(|value| value / 100.0)
            .collect::<Vec<_>>();
        let mut wealth = vec![1.0];
        for period in &returns {
            wealth.push(wealth[wealth.len() - 1] * (1.0 + period));
        }
        let labels = std::iter::once("start".to_owned())
            .chain((0..series.values.len()).map(|index| series.label(index)))
            .collect::<Vec<_>>();
        (wealth, labels, returns)
    };
    let count = returns.len();
    let growth = wealth[wealth.len() - 1] / wealth[0];
    let total = (growth - 1.0) * 100.0;
    let annualized = if growth > 0.0 {
        (growth.powf(per_year as f64 / count as f64) - 1.0) * 100.0
    } else {
        -100.0
    };
    outcome.value("total_return", total, "%")?;
    outcome.value("annualized_return", annualized, "%")?;
    if prices {
        outcome.formulas.push(format!(
            "Total return = last / first − 1 = {} / {} − 1 = {}",
            format::amount(wealth[wealth.len() - 1]),
            format::amount(wealth[0]),
            format::percent(total)
        ));
        outcome.notes.push(
            "Price levels give a price return unless the series is adjusted for dividends and splits."
                .into(),
        );
    } else {
        outcome.formulas.push(format!(
            "Total return = Π(1 + rᵢ) − 1 over {count} returns = {}",
            format::percent(total)
        ));
    }
    outcome.formulas.push(format!(
        "Annualized return = (1 + total)^({per_year} / {count}) − 1 = {}^({per_year} / {count}) − 1 = {}",
        format::amount(growth),
        format::percent(annualized)
    ));
    if count >= 2 {
        let mean = returns.iter().sum::<f64>() / count as f64;
        let variance = returns
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / (count - 1) as f64;
        let deviation = variance.sqrt() * 100.0;
        let scale = (per_year as f64).sqrt();
        let volatility = deviation * scale;
        outcome.value("annualized_volatility", volatility, "%")?;
        outcome.formulas.push(format!(
            "Annualized volatility = sample stdev of {count} period returns × √{per_year} = {} × {} = {}",
            format::percent(deviation),
            format::amount(scale),
            format::percent(volatility)
        ));
    } else {
        outcome
            .notes
            .push("Volatility needs at least two period returns.".into());
    }
    let (mut peak, mut best) = (0, wealth[0]);
    let (mut drawdown, mut drawdown_peak, mut drawdown_trough) = (0.0_f64, 0, 0);
    for (index, value) in wealth.iter().enumerate() {
        if *value > best {
            best = *value;
            peak = index;
        }
        let current = value / best - 1.0;
        if current < drawdown {
            drawdown = current;
            drawdown_peak = peak;
            drawdown_trough = index;
        }
    }
    let drawdown_pct = drawdown * 100.0;
    outcome.value("max_drawdown", drawdown_pct, "%")?;
    if drawdown < 0.0 {
        let recovered = (drawdown_trough + 1..wealth.len())
            .find(|index| wealth[*index] >= wealth[drawdown_peak])
            .map(|index| labels[index].clone());
        outcome.formulas.push(format!(
            "Max drawdown = trough / prior peak − 1 = {} / {} − 1 = {} (peak {}, trough {})",
            format::amount(wealth[drawdown_trough]),
            format::amount(wealth[drawdown_peak]),
            format::percent(drawdown_pct),
            labels[drawdown_peak],
            labels[drawdown_trough]
        ));
        outcome.extra.insert(
            "drawdown".into(),
            json!({
                "peak": labels[drawdown_peak],
                "trough": labels[drawdown_trough],
                "recovered": recovered,
            }),
        );
    } else {
        outcome
            .formulas
            .push("Max drawdown = trough / prior peak − 1 = 0.00% (no decline)".into());
    }
    outcome.extra.insert(
        "observations".into(),
        json!({"period_returns": count, "periods_per_year": per_year}),
    );
    series.record(outcome);
    Ok(())
}

pub(super) fn moving_average(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    if request.field("window").is_none() {
        return Err(CalcError::Missing {
            field: "window".into(),
            expected: "the number of latest values to average, such as 20 or 200".into(),
        });
    }
    let window = request.integer("window", 2, 1_000, 0)? as usize;
    let series = series(request, datasets, outcome, 2)?;
    let count = series.values.len();
    if window > count {
        return Err(CalcError::invalid(
            "window",
            format!("at most the series length ({count})"),
        ));
    }
    let sum = series.values[count - window..].iter().sum::<f64>();
    let average = sum / window as f64;
    let latest = series.values[count - 1];
    let unit = request.amount_unit();
    outcome.value("latest", latest, &unit)?;
    outcome.value("moving_average", average, &unit)?;
    outcome.formulas.push(format!(
        "SMA({window}) = sum of the latest {window} values / {window} = {} / {window} = {}",
        format::amount(sum),
        format::amount(average)
    ));
    if average > 0.0 {
        let difference = (latest / average - 1.0) * 100.0;
        outcome.value("latest_vs_average", difference, "%")?;
        outcome.formulas.push(format!(
            "latest / SMA({window}) − 1 = {} / {} − 1 = {}",
            format::amount(latest),
            format::amount(average),
            format::percent(difference)
        ));
    } else {
        outcome
            .notes
            .push("The average is not positive, so a percent difference is not meaningful.".into());
    }
    outcome
        .extra
        .insert("latest_label".into(), json!(series.label(count - 1)));
    series.record(outcome);
    Ok(())
}

/// Total, count and mean of a series (daily net buying over ten days, say).
/// A missing value fails or, with `skip_missing`, is dropped and counted.
pub(super) fn sum(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let series = series(request, datasets, outcome, 1)?;
    let count = series.values.len();
    let total = series.values.iter().sum::<f64>();
    let mean = total / count as f64;
    let unit = request.amount_unit();
    outcome.value("total", total, &unit)?;
    outcome.value("count", count as f64, "count")?;
    outcome.value("mean", mean, &unit)?;
    let (first, last) = (series.label(0), series.label(count - 1));
    let span = if count == 1 {
        first.clone()
    } else {
        format!("{first} to {last}")
    };
    outcome.formulas.push(format!(
        "Sum of {count} values ({span}) = {}",
        format::amount(total)
    ));
    outcome.formulas.push(format!(
        "Mean = {} / {count} = {}",
        format::amount(total),
        format::amount(mean)
    ));
    series.record(outcome);
    Ok(())
}
