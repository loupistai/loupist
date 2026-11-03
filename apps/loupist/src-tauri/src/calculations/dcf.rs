//! Two-stage DCF sensitivity grid (WACC × terminal growth) and its reverse:
//! the explicit-period growth that the current price implies in each cell.
//! Cash flows are discounted at year end; the terminal value uses the first
//! post-forecast cash flow, FCF_N × (1 + g_T) / (WACC − g_T).
use super::*;

const MAX_AXIS: usize = 9;
const GROWTH_RANGE: (f64, f64) = (-50.0, 100.0);

struct Valuation {
    enterprise: f64,
    explicit: f64,
    terminal_value: f64,
    terminal_present: f64,
    final_flow: f64,
}

fn valuation(
    base: f64,
    growth_pct: f64,
    wacc_pct: f64,
    terminal_pct: f64,
    years: u32,
) -> Valuation {
    let (growth, wacc, terminal) = (growth_pct / 100.0, wacc_pct / 100.0, terminal_pct / 100.0);
    let (mut flow, mut discount, mut explicit) = (base, 1.0, 0.0);
    for _ in 0..years {
        flow *= 1.0 + growth;
        discount *= 1.0 + wacc;
        explicit += flow / discount;
    }
    let terminal_value = flow * (1.0 + terminal) / (wacc - terminal);
    let terminal_present = terminal_value / discount;
    Valuation {
        enterprise: explicit + terminal_present,
        explicit,
        terminal_value,
        terminal_present,
        final_flow: flow,
    }
}

/// Value rises monotonically with growth for a positive base cash flow.
fn implied_growth(base: f64, target: f64, wacc: f64, terminal: f64, years: u32) -> Option<f64> {
    let gap = |growth: f64| valuation(base, growth, wacc, terminal, years).enterprise - target;
    let (mut low, mut high) = GROWTH_RANGE;
    if gap(low) > 0.0 || gap(high) < 0.0 {
        return None;
    }
    for _ in 0..200 {
        let middle = (low + high) / 2.0;
        if gap(middle) < 0.0 {
            low = middle;
        } else {
            high = middle;
        }
        if high - low < 1e-10 {
            break;
        }
    }
    Some((low + high) / 2.0)
}

fn axis(request: &Request, name: &str, minimum: f64, maximum: f64) -> Result<Vec<f64>, CalcError> {
    let items = request
        .field(name)
        .and_then(Value::as_array)
        .ok_or_else(|| CalcError::Missing {
            field: name.into(),
            expected: format!("a list of 1 to {MAX_AXIS} percent values such as [8, 9, 10]"),
        })?;
    if items.is_empty() || items.len() > MAX_AXIS {
        return Err(CalcError::invalid(
            name,
            format!("1 to {MAX_AXIS} percent values"),
        ));
    }
    let mut values = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let field = format!("{name}[{index}]");
        let value = plain_number(item, &field)?;
        if !(value > minimum && value <= maximum) {
            return Err(CalcError::domain(
                &field,
                format!("must be greater than {minimum}% and at most {maximum}%"),
            ));
        }
        if values.contains(&value) {
            return Err(CalcError::invalid(name, "distinct values"));
        }
        values.push(value);
    }
    Ok(values)
}

pub(super) fn grid(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let reverse = request.operation == Operation::ReverseDcf;
    let cash_flow = request.number("cash_flow", datasets)?;
    let base = positive(
        &cash_flow,
        "inputs.cash_flow",
        "must be positive; a growing-perpetuity DCF on a zero or negative base cash flow is not meaningful",
    )?;
    let years = request.integer("forecast_years", 1, 30, 5)? as u32;
    let waccs = axis(request, "wacc_pct", 0.0, 50.0)?;
    let terminals = axis(request, "terminal_growth_pct", -10.0, 20.0)?;
    for wacc in &waccs {
        for terminal in &terminals {
            if wacc <= terminal {
                return Err(CalcError::domain(
                    "wacc_pct",
                    format!(
                        "every WACC must exceed every terminal growth rate; {} ≤ {}",
                        format::rate(*wacc),
                        format::rate(*terminal)
                    ),
                ));
            }
        }
    }
    let net_debt = request.optional_number("net_debt", datasets)?;
    let debt = net_debt.as_ref().map_or(0.0, |number| number.value);
    let shares = request.optional_number("shares", datasets)?;
    if let Some(shares) = &shares {
        positive(shares, "inputs.shares", "must be positive")?;
    }
    let (row, column) = (waccs.len() / 2, terminals.len() / 2);
    let generic = format!(
        "Value = Σ FCF₀(1 + g)^t / (1 + WACC)^t for t = 1..{years} + FCF₀(1 + g)^{years}(1 + g_T) / (WACC − g_T) / (1 + WACC)^{years}; equity = value − net debt"
    );
    outcome.input("cash_flow", &cash_flow);
    if reverse {
        let price = request.number("price", datasets)?;
        let price_value = positive(&price, "inputs.price", "must be positive")?;
        let shares = shares.ok_or_else(|| CalcError::Missing {
            field: "inputs.shares".into(),
            expected: "reverse_dcf needs shares outstanding".into(),
        })?;
        let equity = price_value * shares.value;
        let target = equity + debt;
        if target <= 0.0 {
            return Err(CalcError::domain(
                "inputs.net_debt",
                "price × shares + net debt must be positive to imply a growth rate",
            ));
        }
        let cells = waccs
            .iter()
            .map(|wacc| {
                terminals
                    .iter()
                    .map(|terminal| implied_growth(base, target, *wacc, *terminal, years))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let found = cells
            .iter()
            .flatten()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let outside = cells.iter().flatten().filter(|cell| cell.is_none()).count();
        if outside > 0 {
            outcome.notes.push(format!(
                "{outside} cells imply growth outside {}…{} and are null.",
                format::rate(GROWTH_RANGE.0),
                format::rate(GROWTH_RANGE.1)
            ));
        }
        if let (Some(low), Some(high)) = (
            found.iter().copied().reduce(f64::min),
            found.iter().copied().reduce(f64::max),
        ) {
            outcome.value("implied_growth_low", low, "%")?;
            outcome.value("implied_growth_high", high, "%")?;
        }
        // A reference growth (usually a past CAGR) marks the cells that need more.
        let reference = request.optional_number("reference_growth_pct", datasets)?;
        if let Some(reference) = &reference {
            if !(reference.value > GROWTH_RANGE.0 && reference.value <= GROWTH_RANGE.1) {
                return Err(CalcError::domain(
                    "inputs.reference_growth_pct",
                    format!(
                        "must be greater than {} and at most {}",
                        format::rate(GROWTH_RANGE.0),
                        format::rate(GROWTH_RANGE.1)
                    ),
                ));
            }
        }
        let mut grid = json!({
            "measure": format!("implied annual growth for {years} years"),
            "forecast_years": years,
            "amount_unit": request.unit,
            "unit": "%",
            "rows": "wacc_pct",
            "row_values": waccs,
            "columns": "terminal_growth_pct",
            "column_values": terminals,
            "values": cells.iter().map(|row| row.iter().map(|cell| cell.map(format::round15)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        });
        if let Some(reference) = &reference {
            let above = cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.is_some_and(|growth| growth > reference.value))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let count = above.iter().flatten().filter(|above| **above).count();
            grid["reference"] = json!(format::round15(reference.value));
            grid["above_reference"] = json!(above);
            outcome.notes.push(format!(
                "{count} of {} cells need more yearly growth than the reference {}.",
                waccs.len() * terminals.len(),
                format::percent(reference.value)
            ));
        }
        outcome.extra.insert("grid".into(), grid);
        outcome.formulas.push(format!(
            "{generic}; solve g where value = price × shares + net debt = {} × {} + {} = {}",
            format::amount(price_value),
            format::amount(shares.value),
            format::amount(debt),
            format::amount(target)
        ));
        if let Some(growth) = cells[row][column] {
            outcome.formulas.push(format!(
                "At WACC {} and terminal growth {}: g = {} gives value {}",
                format::rate(waccs[row]),
                format::rate(terminals[column]),
                format::percent(growth),
                format::amount(
                    valuation(base, growth, waccs[row], terminals[column], years).enterprise
                )
            ));
        }
        outcome.input("price", &price);
        outcome.input("shares", &shares);
        if let Some(reference) = &reference {
            outcome.input("reference_growth_pct", reference);
        }
    } else {
        let growth = request.number("growth_pct", datasets)?;
        if !(growth.value > GROWTH_RANGE.0 && growth.value <= GROWTH_RANGE.1) {
            return Err(CalcError::domain(
                "inputs.growth_pct",
                format!(
                    "must be greater than {} and at most {}",
                    format::rate(GROWTH_RANGE.0),
                    format::rate(GROWTH_RANGE.1)
                ),
            ));
        }
        let per_share = |value: f64| shares.as_ref().map_or(value, |shares| value / shares.value);
        let mut values = Vec::with_capacity(waccs.len());
        let mut terminal_shares = Vec::with_capacity(waccs.len());
        for wacc in &waccs {
            let mut row_values = Vec::with_capacity(terminals.len());
            let mut row_shares = Vec::with_capacity(terminals.len());
            for terminal in &terminals {
                let valued = valuation(base, growth.value, *wacc, *terminal, years);
                let cell = per_share(valued.enterprise - debt);
                if !cell.is_finite() {
                    return Err(CalcError::domain(
                        "inputs.cash_flow",
                        "the value is not finite; check the input scale",
                    ));
                }
                row_values.push(format::round15(cell));
                row_shares.push(format::round15(
                    valued.terminal_present / valued.enterprise * 100.0,
                ));
            }
            values.push(row_values);
            terminal_shares.push(row_shares);
        }
        let flat = values.iter().flatten().copied().collect::<Vec<_>>();
        let unit = if shares.is_some() {
            "per share".to_owned()
        } else {
            request.amount_unit()
        };
        let low = flat.iter().copied().fold(f64::INFINITY, f64::min);
        let high = flat.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        outcome.value("value_low", low, &unit)?;
        outcome.value("value_high", high, &unit)?;
        outcome.extra.insert(
            "grid".into(),
            json!({
                "measure": if shares.is_some() { "equity value per share" } else { "equity value" },
                "forecast_years": years,
                "amount_unit": request.unit,
                "unit": unit,
                "rows": "wacc_pct",
                "row_values": waccs,
                "columns": "terminal_growth_pct",
                "column_values": terminals,
                "values": values,
                "terminal_value_share_pct": terminal_shares,
            }),
        );
        outcome.formulas.push(format!(
            "{generic}, with FCF₀ = {}, g = {}",
            format::amount(base),
            format::rate(growth.value)
        ));
        let example = valuation(base, growth.value, waccs[row], terminals[column], years);
        let equity = example.enterprise - debt;
        let mut line = format!(
            "At WACC {} and terminal growth {}: forecast PV = {}; terminal value = {} × (1 + {}) / ({} − {}) = {}, PV = {}; value = {}; equity = {} − {} = {}",
            format::rate(waccs[row]),
            format::rate(terminals[column]),
            format::amount(example.explicit),
            format::amount(example.final_flow),
            format::rate(terminals[column]),
            format::rate(waccs[row]),
            format::rate(terminals[column]),
            format::amount(example.terminal_value),
            format::amount(example.terminal_present),
            format::amount(example.enterprise),
            format::amount(example.enterprise),
            format::amount(debt),
            format::amount(equity)
        );
        if let Some(shares) = &shares {
            line.push_str(&format!(
                "; per share = {} / {} = {}",
                format::amount(equity),
                format::amount(shares.value),
                format::amount(equity / shares.value)
            ));
        }
        outcome.formulas.push(line);
        outcome.input("growth_pct", &growth);
        if let Some(shares) = &shares {
            outcome.input("shares", shares);
        }
    }
    if let Some(net_debt) = &net_debt {
        outcome.input("net_debt", net_debt);
    }
    outcome.notes.push(format!(
        "Year-end discounting over {years} forecast years; amounts, price × shares and net debt must share one currency and scale."
    ));
    Ok(())
}
