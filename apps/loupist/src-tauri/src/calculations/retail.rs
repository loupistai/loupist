//! Everyday investor arithmetic: how much two funds hold in common, where
//! regular saving could lead across several return rates (never one), and
//! what regular buying over a dated price history would have done.
use super::*;
use std::collections::BTreeMap;

const MAX_WEIGHTS: usize = 500;
/// Weights may add to slightly over 100% from rounding in fund reports.
const MAX_WEIGHT_TOTAL: f64 = 100.5;
const TOP_SHARED: usize = 10;
const MAX_RATES: usize = 9;
const RATE_RANGE: (f64, f64) = (-20.0, 30.0);
const PERIODS_PER_YEAR: [u64; 4] = [1, 4, 12, 52];

/// `{id: weight %}` with 1–500 holdings whose weights add to at most 100.5%.
fn weights(request: &Request, field: &str) -> Result<BTreeMap<String, f64>, CalcError> {
    let object = request
        .field(field)
        .and_then(Value::as_object)
        .ok_or_else(|| CalcError::Missing {
            field: field.into(),
            expected: "an object of holding ids to weights in percent, such as {\"AAPL\": 6.8}"
                .into(),
        })?;
    if object.is_empty() || object.len() > MAX_WEIGHTS {
        return Err(CalcError::invalid(
            field,
            format!("1 to {MAX_WEIGHTS} holdings"),
        ));
    }
    let mut out = BTreeMap::new();
    for (id, weight) in object {
        let key = format!("{field}.{id}");
        if id.trim().is_empty()
            || id.chars().count() > MAX_LABEL_CHARS
            || id.chars().any(char::is_control)
        {
            return Err(CalcError::invalid(
                field,
                format!("holding ids of 1 to {MAX_LABEL_CHARS} characters"),
            ));
        }
        let weight = plain_number(weight, &key)?;
        if !(0.0..=100.0).contains(&weight) {
            return Err(CalcError::domain(&key, "a weight in percent from 0 to 100"));
        }
        out.insert(id.trim().to_uppercase(), weight);
    }
    if out.len() != object.len() {
        return Err(CalcError::invalid(
            field,
            "distinct holding ids (ids are compared without case)",
        ));
    }
    let total = out.values().sum::<f64>();
    if total > MAX_WEIGHT_TOTAL {
        return Err(CalcError::domain(
            field,
            format!(
                "weights add to {}; a fund's weights add to at most {MAX_WEIGHT_TOTAL}%",
                format::rate(total)
            ),
        ));
    }
    Ok(out)
}

/// Overlap = Σ min(weight in A, weight in B) over the holdings both list.
pub(super) fn holdings_overlap(request: &Request, outcome: &mut Outcome) -> Result<(), CalcError> {
    let a = weights(request, "weights_a")?;
    let b = weights(request, "weights_b")?;
    let (label_a, label_b) = (request.label("weights_a"), request.label("weights_b"));
    let mut shared: Vec<(&String, f64, f64)> = a
        .iter()
        .filter_map(|(id, weight)| b.get(id).map(|other| (id, *weight, *other)))
        .collect();
    let overlap = shared.iter().map(|(_, x, y)| x.min(*y)).sum::<f64>();
    shared.sort_by(|left, right| {
        right
            .1
            .min(right.2)
            .total_cmp(&left.1.min(left.2))
            .then(left.0.cmp(right.0))
    });
    outcome.value("overlap", overlap, "%")?;
    outcome.value("shared_holdings", shared.len() as f64, "count")?;
    outcome.formulas.push(format!(
        "Overlap = Σ min(weight in {label_a}, weight in {label_b}) over {} shared holdings = {}",
        shared.len(),
        format::percent(overlap)
    ));
    if let Some((id, x, y)) = shared.first() {
        outcome.formulas.push(format!(
            "Largest shared: {id} = min({}, {}) = {}",
            format::rate(*x),
            format::rate(*y),
            format::rate(x.min(*y))
        ));
    }
    outcome.extra.insert(
        "top_shared".into(),
        json!(shared
            .iter()
            .take(TOP_SHARED)
            .map(|(id, x, y)| json!({
                "id": id,
                "weight_a": format::round15(*x),
                "weight_b": format::round15(*y),
                "overlap": format::round15(x.min(*y)),
            }))
            .collect::<Vec<_>>()),
    );
    for (name, set) in [("weights_a", &a), ("weights_b", &b)] {
        outcome.inputs.push(json!({
            "name": name,
            "count": set.len(),
            "total_weight": format::round15(set.values().sum::<f64>()),
            "display": format!("{} holdings", set.len()),
            "provenance": {"kind": "provided"},
        }));
    }
    outcome.notes.push(
        "Holdings are matched by id as given; the same company listed under two ids is not matched."
            .into(),
    );
    Ok(())
}

fn rates(request: &Request) -> Result<Vec<f64>, CalcError> {
    let items = request
        .field("return_pct")
        .and_then(Value::as_array)
        .ok_or_else(|| CalcError::Missing {
            field: "return_pct".into(),
            expected: format!(
                "a list of 1 to {MAX_RATES} yearly return rates in percent, such as [3, 5, 7]"
            ),
        })?;
    if items.is_empty() || items.len() > MAX_RATES {
        return Err(CalcError::invalid(
            "return_pct",
            format!("1 to {MAX_RATES} rates"),
        ));
    }
    let mut values = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let field = format!("return_pct[{index}]");
        let value = plain_number(item, &field)?;
        if !(RATE_RANGE.0..=RATE_RANGE.1).contains(&value) {
            return Err(CalcError::domain(
                &field,
                format!(
                    "a yearly return from {} to {}",
                    format::rate(RATE_RANGE.0),
                    format::rate(RATE_RANGE.1)
                ),
            ));
        }
        if values.contains(&value) {
            return Err(CalcError::invalid("return_pct", "distinct rates"));
        }
        values.push(value);
    }
    Ok(values)
}

/// Growth of one unit over `n` periods and the end value of one deposit per
/// period, both at periodic rate `i`.
fn factors(i: f64, n: f64) -> (f64, f64) {
    let growth = (1.0 + i).powf(n);
    let annuity = if i.abs() < 1e-12 {
        n
    } else {
        (growth - 1.0) / i
    };
    (growth, annuity)
}

/// Forward (a regular contribution) or backward (a target): one value per
/// yearly return rate, end-of-period deposits, nominal and before costs.
pub(super) fn goal_grid(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let years = request.number("years", datasets)?;
    if !(years.value >= 1.0 && years.value <= 60.0 && years.value.fract() == 0.0) {
        return Err(CalcError::domain(
            "inputs.years",
            "a whole number of years from 1 to 60",
        ));
    }
    let per_year = request.integer("periods_per_year", 1, 52, 12)?;
    if !PERIODS_PER_YEAR.contains(&per_year) {
        return Err(CalcError::invalid("periods_per_year", "1, 4, 12 or 52"));
    }
    let start = request.optional_number("start", datasets)?;
    let start_value = start.as_ref().map_or(0.0, |start| start.value);
    if start_value < 0.0 {
        return Err(CalcError::domain(
            "inputs.start",
            "must be zero or positive",
        ));
    }
    let contribution = request.optional_number("contribution", datasets)?;
    let target = request.optional_number("target", datasets)?;
    let forward = match (&contribution, &target) {
        (Some(contribution), None) => {
            if contribution.value < 0.0 {
                return Err(CalcError::domain(
                    "inputs.contribution",
                    "must be zero or positive",
                ));
            }
            true
        }
        (None, Some(target)) => {
            positive(target, "inputs.target", "must be positive")?;
            false
        }
        _ => {
            return Err(CalcError::invalid(
                "inputs.contribution",
                "either contribution (where saving leads) or target (what to save each period), not both",
            ))
        }
    };
    let rates = rates(request)?;
    let periods = years.value * per_year as f64;
    let unit = request.amount_unit();
    let mut rows = Vec::with_capacity(rates.len());
    let mut results = Vec::with_capacity(rates.len());
    for rate in &rates {
        let periodic = (1.0 + rate / 100.0).powf(1.0 / per_year as f64) - 1.0;
        let (growth, annuity) = factors(periodic, periods);
        let result = if forward {
            let deposit = contribution.as_ref().map_or(0.0, |c| c.value);
            start_value * growth + deposit * annuity
        } else {
            let goal = target.as_ref().map_or(0.0, |t| t.value);
            ((goal - start_value * growth) / annuity).max(0.0)
        };
        if !result.is_finite() {
            return Err(CalcError::domain(
                "return_pct",
                "the result is not a finite number; check the input scale",
            ));
        }
        results.push(result);
        let mut row = Map::new();
        row.insert("return_pct".into(), json!(rate));
        row.insert(
            "periodic_rate_pct".into(),
            json!(format::round15(periodic * 100.0)),
        );
        row.insert(
            if forward { "value" } else { "contribution" }.into(),
            json!(format::round15(result)),
        );
        row.insert("display".into(), json!(format::amount(result)));
        rows.push(Value::Object(row));
    }
    let low = results.iter().copied().fold(f64::INFINITY, f64::min);
    let high = results.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (low_name, high_name) = if forward {
        ("value_low", "value_high")
    } else {
        ("contribution_low", "contribution_high")
    };
    outcome.value(low_name, low, &unit)?;
    outcome.value(high_name, high, &unit)?;
    if forward {
        let deposit = contribution.as_ref().map_or(0.0, |c| c.value);
        outcome.value("total_contributed", start_value + deposit * periods, &unit)?;
    }
    outcome.extra.insert(
        "grid".into(),
        json!({
            "measure": if forward { "value at the end" } else { "contribution each period" },
            "unit": unit,
            "periods_per_year": per_year,
            "periods": periods,
            "rows": rows,
        }),
    );
    let middle = rates.len() / 2;
    let periodic = (1.0 + rates[middle] / 100.0).powf(1.0 / per_year as f64) - 1.0;
    let (growth, annuity) = factors(periodic, periods);
    if forward {
        let deposit = contribution.as_ref().map_or(0.0, |c| c.value);
        outcome.formulas.push(format!(
            "Value = start × (1 + i)^n + contribution × ((1 + i)^n − 1) / i, i = (1 + r)^(1/{per_year}) − 1, n = {} × {per_year} = {}; deposits at the end of each period",
            format::amount(years.value),
            format::amount(periods)
        ));
        outcome.formulas.push(format!(
            "At {} a year: i = {}; value = {} × {} + {} × {} = {}",
            format::rate(rates[middle]),
            format::percent(periodic * 100.0),
            format::amount(start_value),
            format::amount(growth),
            format::amount(deposit),
            format::amount(annuity),
            format::amount(results[middle])
        ));
    } else {
        let goal = target.as_ref().map_or(0.0, |t| t.value);
        outcome.formulas.push(format!(
            "Contribution = (target − start × (1 + i)^n) × i / ((1 + i)^n − 1), i = (1 + r)^(1/{per_year}) − 1, n = {} × {per_year} = {}; deposits at the end of each period",
            format::amount(years.value),
            format::amount(periods)
        ));
        outcome.formulas.push(format!(
            "At {} a year: i = {}; contribution = ({} − {} × {}) / {} = {}",
            format::rate(rates[middle]),
            format::percent(periodic * 100.0),
            format::amount(goal),
            format::amount(start_value),
            format::amount(growth),
            format::amount(annuity),
            format::amount(results[middle])
        ));
        if results.contains(&0.0) {
            outcome.notes.push(
                "At some rates the starting amount alone reaches the target, so the contribution is 0."
                    .into(),
            );
        }
    }
    outcome.input("years", &years);
    if let Some(start) = &start {
        outcome.input("start", start);
    }
    if let Some(contribution) = &contribution {
        outcome.input("contribution", contribution);
    }
    if let Some(target) = &target {
        outcome.input("target", target);
    }
    outcome.notes.push(
        "Nominal amounts before costs and taxes; the return rates are assumptions, not forecasts."
            .into(),
    );
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Schedule {
    Monthly,
    Weekly,
    EveryRow,
}

/// The period a dated row falls in, or `None` when the label is not a date
/// precise enough for the schedule.
fn period_of(label: &str, schedule: Schedule) -> Option<(i32, u32)> {
    let key = series::date_key(label)?;
    let digits: String = key.chars().take(8).collect();
    match schedule {
        Schedule::EveryRow => Some((0, 0)),
        Schedule::Monthly if digits.len() >= 6 => {
            Some((digits[..4].parse().ok()?, digits[4..6].parse().ok()?))
        }
        Schedule::Weekly if digits.len() == 8 => {
            let date = chrono::NaiveDate::parse_from_str(&digits, "%Y%m%d").ok()?;
            let week = chrono::Datelike::iso_week(&date);
            Some((week.year(), week.week()))
        }
        _ => None,
    }
}

/// Buying a fixed amount on the first trading row of each month or week (or
/// every row) of a dated price series, with an optional fee per purchase.
pub(super) fn dca_history(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let schedule = match request.field("schedule") {
        None => Schedule::Monthly,
        Some(value) => match value.as_str() {
            Some("monthly") => Schedule::Monthly,
            Some("weekly") => Schedule::Weekly,
            Some("every_row") => Schedule::EveryRow,
            _ => {
                return Err(CalcError::invalid(
                    "schedule",
                    "monthly, weekly or every_row",
                ))
            }
        },
    };
    let contribution = request.number("contribution", datasets)?;
    let amount = positive(&contribution, "inputs.contribution", "must be positive")?;
    let fee = request.optional_number("fee_pct", datasets)?;
    let fee_pct = fee.as_ref().map_or(0.0, |fee| fee.value);
    if !(0.0..100.0).contains(&fee_pct) {
        return Err(CalcError::domain(
            "inputs.fee_pct",
            "a fee in percent from 0 to below 100",
        ));
    }
    let series = series::series(request, datasets, outcome, 1)?;
    let Some(labels) = series.labels.as_ref() else {
        return Err(CalcError::Missing {
            field: "dates".into(),
            expected: "a date for every price (dates, or series.date_column for a dataset)".into(),
        });
    };
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
    let mut last_period = None;
    let (mut invested, mut units, mut purchases) = (0.0_f64, 0.0_f64, 0_usize);
    let (mut first_buy, mut last_buy) = (None, None);
    for (index, price) in series.values.iter().enumerate() {
        let period = period_of(&labels[index], schedule).ok_or_else(|| {
            CalcError::invalid(
                &format!("dates[{index}]"),
                match schedule {
                    Schedule::Weekly => "a calendar date such as 2026-03-02 for a weekly schedule",
                    _ => "a calendar date such as 2026-03-02 or a month such as 2026-03",
                },
            )
        })?;
        if schedule != Schedule::EveryRow && last_period == Some(period) {
            continue;
        }
        last_period = Some(period);
        invested += amount;
        units += amount * (1.0 - fee_pct / 100.0) / price;
        purchases += 1;
        first_buy.get_or_insert(index);
        last_buy = Some(index);
    }
    let last_index = series.values.len() - 1;
    let last_price = series.values[last_index];
    let value = units * last_price;
    let average = invested / units;
    let gain = (value / invested - 1.0) * 100.0;
    let unit = request.amount_unit();
    outcome.value("invested", invested, &unit)?;
    outcome.value("units", units, "units")?;
    outcome.value("average_cost", average, &unit)?;
    outcome.value("last_price", last_price, &unit)?;
    outcome.value("value", value, &unit)?;
    outcome.value("return", gain, "%")?;
    let name = match schedule {
        Schedule::Monthly => "the first trading row of each month",
        Schedule::Weekly => "the first trading row of each week",
        Schedule::EveryRow => "every row",
    };
    outcome.formulas.push(format!(
        "Bought {} on {name}: {purchases} purchases from {} to {}; invested = {} × {purchases} = {}",
        format::amount(amount),
        series.label(first_buy.unwrap_or(0)),
        series.label(last_buy.unwrap_or(0)),
        format::amount(amount),
        format::amount(invested)
    ));
    outcome.formulas.push(if fee_pct > 0.0 {
        format!(
            "Units = Σ contribution × (1 − {}) / price = {}",
            format::rate(fee_pct),
            format::amount(units)
        )
    } else {
        format!("Units = Σ contribution / price = {}", format::amount(units))
    });
    outcome.formulas.push(format!(
        "Value = units × last price ({}) = {} × {} = {}; average cost = {} / {} = {}",
        series.label(last_index),
        format::amount(units),
        format::amount(last_price),
        format::amount(value),
        format::amount(invested),
        format::amount(units),
        format::amount(average)
    ));
    outcome.formulas.push(format!(
        "Return = value / invested − 1 = {} / {} − 1 = {}",
        format::amount(value),
        format::amount(invested),
        format::percent(gain)
    ));
    outcome.extra.insert(
        "purchases".into(),
        json!({"count": purchases, "schedule": match schedule {
            Schedule::Monthly => "monthly",
            Schedule::Weekly => "weekly",
            Schedule::EveryRow => "every_row",
        }}),
    );
    outcome.input("contribution", &contribution);
    if let Some(fee) = &fee {
        outcome.input("fee_pct", fee);
    }
    series.record(outcome);
    outcome.notes.push(
        "Prices as given: a series without dividends gives a price return; before taxes and without later costs."
            .into(),
    );
    Ok(())
}
