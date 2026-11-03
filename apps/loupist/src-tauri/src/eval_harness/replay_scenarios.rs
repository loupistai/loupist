//! Model-free calls through the native broker, credentials seam and provider parsers.
use super::*;

fn call(name: &str, method: &str, args: Value, expected: &[&str]) -> (Value, Vec<String>) {
    (
        json!({"name":name,"method":method,"args":args}),
        expected.iter().map(|s| (*s).into()).collect(),
    )
}

/// A read the turn's as-of cutoff must refuse; `absent` never reaches the answer.
fn after_cutoff(name: &str, method: &str, args: Value, absent: &[&str]) -> (Value, Vec<String>) {
    let mut case = call(name, method, args, &["as-of cutoff"]);
    case.0["error"] = json!("as-of cutoff");
    case.0["absent"] = json!(absent);
    case
}

#[tokio::test]
async fn eval_replay_tool_use_scenarios_through_native_broker() {
    let suite: Value =
        serde_json::from_str(include_str!("../../../evals/agent/suites/tool-use-v1.json")).unwrap();
    for scenario in suite["scenarios"].as_array().unwrap().iter().filter(|s| {
        s["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("needs-replay"))
    }) {
        let id = scenario["id"].as_str().unwrap();
        let root = tempfile::Builder::new()
            .prefix("eval-replay-cases-")
            .tempdir_in("/tmp")
            .unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        super::tests::scripted(&mut state, root.path());
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../evals/agent/suites")
            .join(scenario["replay"].as_str().unwrap());
        let replay: replay::Replay = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let _guard = replay.install(&state.credential_environment).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let connections: Vec<String> =
            serde_json::from_value(scenario["connections"].clone()).unwrap();
        enable_connections(&state, &loupist.id, &connections).unwrap();
        let as_of = scenario["sessions"][0]["turns"][0]["as_of"]
            .as_str()
            .map(str::to_owned);
        let mut cases = Vec::new();
        match id {
            "T-017" | "T-026" => {
                let (name, code, stock, basis, revenue, income) = if id == "T-017" {
                    (
                        "솔숲식품",
                        "99022100",
                        "990221",
                        "consolidated",
                        "452300000000",
                        "38100000000",
                    )
                } else {
                    (
                        "이슬물류",
                        "99055200",
                        "990552",
                        "separate",
                        "128400000000",
                        "7600000000",
                    )
                };
                for (query, limit) in [(name, 1), (name, 8), (stock, 20)] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query,"limit":limit}),
                        &[code, stock],
                    ));
                }
                for company in [code, stock, name] {
                    cases.push(call("dart_financials", "dart.financials", json!({"company":company,"fiscal_year":2025,"report_period":"annual","basis":basis}), &[revenue, income, basis]));
                }
                let wrong_basis = if basis == "separate" {
                    "consolidated"
                } else {
                    "separate"
                };
                let mut negative = call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":code,"fiscal_year":2025,"report_period":"annual","basis":wrong_basis}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-018" => {
                for query in ["별무리반도체", "990308"] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query}),
                        &["99030800"],
                    ));
                }
                for (company, start, end, limit, class) in [
                    ("990308", "2026-01-01", "2026-03-31", 20, None),
                    (
                        "99030800",
                        "2026-01-01",
                        "2026-09-30",
                        100,
                        Some("major_event"),
                    ),
                    (
                        "별무리반도체",
                        "2026-03-18",
                        "2026-03-18",
                        1,
                        Some("exchange"),
                    ),
                ] {
                    let mut args = json!({"company":company,"start":start,"end":end,"limit":limit});
                    if let Some(class) = class {
                        args["filing_type"] = json!(class);
                    }
                    cases.push(call(
                        "dart_filing_search",
                        "dart.filing_search",
                        args,
                        &["20260318990308", "포장라인"],
                    ));
                }
                cases.push(call(
                    "dart_filing_read",
                    "dart.filing_read",
                    json!({"rcept_no":"20260318990308"}),
                    &["3 packaging lines", "포장 라인 3개"],
                ));
                let mut negative = call(
                    "dart_filing_search",
                    "dart.filing_search",
                    json!({"company":"99030800","start":"2026-04-01","end":"2026-06-30","limit":20}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-019" | "T-020" => {
                let (symbol, date, price, volume) = if id == "T-019" {
                    ("990184", "2026-03-03", "73400", "182233")
                } else {
                    ("990221", "2026-04-16", "21950", "64012")
                };
                cases.push(call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":symbol,"date":date}),
                    &[price, volume],
                ));
                // Missing issue is rejected by the real KRX row selector.
                let mut negative = call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"999999","date":date}),
                    &["no matching data"],
                );
                negative.0["error"] = json!("no matching data");
                cases.push(negative);
            }
            "T-021" => {
                for query in ["inquire_price", "주식현재가 시세"] {
                    cases.push(call(
                        "kis_operation_search",
                        "kis.operation_search",
                        json!({"query":query,"product":"domestic_stock","limit":20}),
                        &["domestic_stock.inquire_price", "fid_input_iscd"],
                    ));
                }
                for market in ["J", "NX", "UN"] {
                    cases.push(call("kis_read", "kis.read", json!({"operation_id":"domestic_stock.inquire_price","params":{"fid_cond_mrkt_div_code":market,"fid_input_iscd":"990417"}}), &["51200", "990417"]));
                }
                let mut negative = call(
                    "kis_read",
                    "kis.read",
                    json!({"operation_id":"domestic_stock.inquire_price","params":{"fid_cond_mrkt_div_code":"J","fid_input_iscd":"999999"}}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-022" | "T-023" => {
                let (indicator, year, value, prior) = if id == "T-022" {
                    ("GT.FICT.MARR.IDX", 2023, "109.25", "101.5")
                } else {
                    ("GT.FICT.FRT.IDX", 2024, "88.4", "81.2")
                };
                for (economy, start, end) in [
                    ("XQ", year, year),
                    ("XQ", year - 1, year),
                    ("xq", 2000, 2026),
                ] {
                    let expected = if start < year {
                        vec![value, prior]
                    } else {
                        vec![value]
                    };
                    cases.push(call("worldbank_series", "worldbank.series", json!({"economy":economy,"indicator":indicator.to_lowercase(),"start_year":start,"end_year":end}), &expected));
                }
                for (economy, start, end) in [("XQ", year - 2, year - 1), ("ZZ", year, year)] {
                    let mut negative = call(
                        "worldbank_series",
                        "worldbank.series",
                        json!({"economy":economy,"indicator":indicator,"start_year":start,"end_year":end}),
                        &["eval_replay_unmatched"],
                    );
                    negative.0["error"] = json!("eval_replay_unmatched");
                    cases.push(negative);
                }
            }
            "T-024" => {
                for (query, limit) in [("노을제지", 1), ("노을제지", 8), ("990184", 20)] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query,"limit":limit}),
                        &["990184", "99018400"],
                    ));
                }
            }
            "T-025" => {
                for query in ["cash", "잔고", "inquire_balance"] {
                    cases.push(call(
                        "kis_operation_search",
                        "kis.operation_search",
                        json!({"query":query,"limit":20}),
                        &[
                            "\"account_reads_available_v1\": 0",
                            "\"orders_available\": false",
                        ],
                    ));
                }
                let mut negative = call(
                    "kis_read",
                    "kis.read",
                    json!({"operation_id":"domestic_stock.inquire_balance","params":{}}),
                    &["profile"],
                );
                negative.0["error"] = json!("profile");
                cases.push(negative);
            }
            "T-032" => {
                // A daily bar is known at the close of its date: the cutoff
                // day's close stays, the next day's never reaches the answer.
                let mut range = call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"990184","start":"2026-03-03","end":"2026-03-04"}),
                    &["73400", "\"post_cutoff_excluded\": 1"],
                );
                range.0["absent"] = json!(["74650", "2026-03-04\""]);
                cases.push(range);
                cases.push(after_cutoff(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"990184","date":"2026-03-04"}),
                    &["74650"],
                ));
            }
            "T-033" => {
                cases.push(call(
                    "dart_company_lookup",
                    "dart.company_lookup",
                    json!({"query":"990221"}),
                    &["99022100"],
                ));
                cases.push(after_cutoff(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["452300000000"],
                ));
            }
            "T-034" => {
                let mut search = call(
                    "dart_filing_search",
                    "dart.filing_search",
                    json!({"company":"990308","start":"2026-01-01","end":"2026-03-31","limit":20}),
                    &["20260305990308"],
                );
                search.0["absent"] = json!(["20260318990308", "포장"]);
                cases.push(search);
                cases.push(call(
                    "dart_filing_read",
                    "dart.filing_read",
                    json!({"rcept_no":"20260305990308"}),
                    &["2 inspection machines"],
                ));
                cases.push(after_cutoff(
                    "dart_filing_read",
                    "dart.filing_read",
                    json!({"rcept_no":"20260318990308"}),
                    &["packaging"],
                ));
            }
            "T-035" => {
                cases.push(call(
                    "web_fetch",
                    "web.fetch",
                    json!({"url":"https://fictional.example/noeul-paper/mill-update-march-2"}),
                    &["410 thousand tonnes"],
                ));
                cases.push(after_cutoff(
                    "web_fetch",
                    "web.fetch",
                    json!({"url":"https://fictional.example/noeul-paper/mill-update-march-18"}),
                    &["455"],
                ));
            }
            "T-036" | "T-037" | "T-038" | "T-039" => {
                // Show the math and the as-of Memory cases read the same filed statements.
                cases.push(call(
                    "dart_company_lookup",
                    "dart.company_lookup",
                    json!({"query":"990221","limit":20}),
                    &["99022100"],
                ));
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["452300000000", "38100000000", "consolidated"],
                ));
            }
            "T-040" => {
                // A current-only quote is refused under the as-of date before any request.
                cases.push(after_cutoff(
                    "kis_read",
                    "kis.read",
                    json!({"operation_id":"domestic_stock.inquire_price","params":{"fid_cond_mrkt_div_code":"J","fid_input_iscd":"990417"}}),
                    &["51200"],
                ));
            }
            "T-041" | "T-042" => {
                // Grid trio: Iseul files separate statements only; T-042 has no Hanbit statements.
                for (query, code) in [
                    ("솔숲식품", "99022100"),
                    ("이슬물류", "99055200"),
                    ("990777", "99077700"),
                ] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query,"limit":8}),
                        &[code],
                    ));
                }
                let financials = |company: &str, basis: &str| json!({"company":company,"fiscal_year":2025,"report_period":"annual","basis":basis});
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    financials("99022100", "consolidated"),
                    &["452300000000", "38100000000", "21400000000"],
                ));
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    financials("99055200", "separate"),
                    &["128400000000", "7600000000", "4900000000", "separate"],
                ));
                let mut no_data = |company: &str, basis: &str| {
                    let mut case = call(
                        "dart_financials",
                        "dart.financials",
                        financials(company, basis),
                        &["no matching data"],
                    );
                    case.0["error"] = json!("no matching data");
                    cases.push(case);
                };
                no_data("99055200", "consolidated");
                if id == "T-041" {
                    cases.push(call(
                        "dart_financials",
                        "dart.financials",
                        financials("99077700", "consolidated"),
                        &["286500000000", "31400000000", "19800000000"],
                    ));
                } else {
                    no_data("99077700", "consolidated");
                    no_data("99077700", "separate");
                }
            }
            "T-051" => {
                // The filed annual figure and a news page with the preliminary one.
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["452300000000", "consolidated"],
                ));
                cases.push(call(
                    "web_fetch",
                    "web.fetch",
                    json!({"url":"https://fictional.example/markets/solsup-foods-preliminary-fy2025"}),
                    &["455.0 billion won"],
                ));
            }
            "T-044" => {
                // One convertible bond decision, its filing, and no decision of the other kinds.
                for query in ["별무리", "990308"] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query}),
                        &["99030800"],
                    ));
                }
                let decisions = |company: &str, kind: &str, start: &str, end: &str| json!({"company":company,"kind":kind,"start":start,"end":end});
                cases.push(call(
                    "dart_major_report",
                    "dart.major_report",
                    decisions("99030800", "convertible_bonds", "2026-01-01", "2026-10-31"),
                    &[
                        "loupist-dart-major-report/1",
                        "20261030990308",
                        "9100",
                        "6370",
                        "2026-11-05",
                        "\"dilution_filed_pct\": \"6.1\"",
                        "\"max_new_shares\": \"4709576\"",
                    ],
                ));
                cases.push(call(
                    "dart_major_report",
                    "dart.major_report",
                    decisions(
                        "별무리반도체",
                        "convertible_bonds",
                        "2026-10-30",
                        "2026-10-30",
                    ),
                    &["9100", "6370"],
                ));
                for kind in [
                    "bonds_with_warrants",
                    "exchangeable_bonds",
                    "rights_offering",
                ] {
                    cases.push(call(
                        "dart_major_report",
                        "dart.major_report",
                        decisions("99030800", kind, "2026-01-01", "2026-10-31"),
                        &["\"rows\": []"],
                    ));
                }
                cases.push(call(
                    "dart_filing_search",
                    "dart.filing_search",
                    json!({"company":"99030800","start":"2026-10-01","end":"2026-10-31","limit":20,"filing_type":"major_event"}),
                    &["20261030990308", "전환사채권발행결정"],
                ));
                cases.push(call(
                    "dart_filing_read",
                    "dart.filing_read",
                    json!({"rcept_no":"20261030990308"}),
                    &["가상성장투자조합", "최저 조정가액"],
                ));
                let mut negative = call(
                    "dart_major_report",
                    "dart.major_report",
                    decisions("99030800", "buyback", "2026-01-01", "2026-10-31"),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-045" => {
                // Investor flows, short selling and lending for one issue; no credit balances.
                for query in [
                    "investor_trade_by_stock_daily",
                    "daily_short_sale",
                    "daily_loan_trans",
                ] {
                    let operation = format!("domestic_stock.{query}");
                    cases.push(call(
                        "kis_operation_search",
                        "kis.operation_search",
                        json!({"query":query,"product":"domestic_stock","limit":20}),
                        &[operation.as_str()],
                    ));
                }
                for (market, date) in [("J", "20261030"), ("UN", "20261031")] {
                    cases.push(call("kis_read", "kis.read", json!({"operation_id":"domestic_stock.investor_trade_by_stock_daily","params":{"fid_cond_mrkt_div_code":market,"fid_input_iscd":"990417","fid_input_date_1":date,"fid_org_adj_prc":"","fid_etc_cls_code":""}}), &["frgn_ntby_tr_pbmn", "20261019", "2140", "-1510"]));
                }
                cases.push(call("kis_read", "kis.read", json!({"operation_id":"domestic_stock.daily_short_sale","params":{"fid_cond_mrkt_div_code":"J","fid_input_iscd":"990417","fid_input_date_1":"20261001","fid_input_date_2":"20261030"}}), &["ssts_cntg_qty", "17900", "412300"]));
                cases.push(call("kis_read", "kis.read", json!({"operation_id":"domestic_stock.daily_loan_trans","params":{"mrkt_div_cls_code":"3","mksc_shrn_iscd":"990417","start_date":"20261001","end_date":"20261030"}}), &["rmnd_stcn", "1986200", "2049800"]));
                let mut negative = call(
                    "kis_read",
                    "kis.read",
                    json!({"operation_id":"domestic_stock.investor_trade_by_stock_daily","params":{"fid_cond_mrkt_div_code":"J","fid_input_iscd":"999999","fid_input_date_1":"20261030","fid_org_adj_prc":"","fid_etc_cls_code":""}}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-046" => {
                // No consolidated statements: the separate ones answer.
                cases.push(call(
                    "dart_company_lookup",
                    "dart.company_lookup",
                    json!({"query":"이슬물류"}),
                    &["99055200", "990552"],
                ));
                let financials = |basis: &str| json!({"company":"99055200","fiscal_year":2025,"report_period":"annual","basis":basis});
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    financials("separate"),
                    &["128400000000", "7600000000", "separate"],
                ));
                let mut consolidated = call(
                    "dart_financials",
                    "dart.financials",
                    financials("consolidated"),
                    &["no matching data"],
                );
                consolidated.0["error"] = json!("no matching data");
                cases.push(consolidated);
            }
            "T-047" => {
                // Four annual dividend tables; only FY2025 has a readable decision notice.
                cases.push(call(
                    "dart_company_lookup",
                    "dart.company_lookup",
                    json!({"query":"솔숲식품"}),
                    &["99022100"],
                ));
                cases.push(call(
                    "dart_dividends",
                    "dart.dividends",
                    json!({"company":"99022100","year":2025}),
                    &["dps_cash", "28.6", "\"status\": \"found\"", "2025-12-31"],
                ));
                for year in [2024, 2023, 2022] {
                    cases.push(call(
                        "dart_dividends",
                        "dart.dividends",
                        json!({"company":"99022100","year":year}),
                        &["dps_cash", "payout_ratio", "\"status\": \"no_notice\""],
                    ));
                }
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["52600000000", "-18900000000", "21000000000"],
                ));
                let mut negative = call(
                    "dart_dividends",
                    "dart.dividends",
                    json!({"company":"99022100","year":2021}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-049" => {
                // The close before the move sits across a substitute holiday; Seollal has no rows.
                cases.push(call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"990184","start":"2026-02-26","end":"2026-03-04"}),
                    &["72500", "73400", "74650", "2026-03-02"],
                ));
                cases.push(call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"990184","start":"2026-02-16","end":"2026-03-06"}),
                    &["70800", "74300", "2026-02-17"],
                ));
                cases.push(call(
                    "web_fetch",
                    "web.fetch",
                    json!({"url":"https://fictional.example/noeul-paper/mill-update-march-2"}),
                    &["410 thousand tonnes"],
                ));
                let mut negative = call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"999999","date":"2026-03-03"}),
                    &["no matching data"],
                );
                negative.0["error"] = json!("no matching data");
                cases.push(negative);
            }
            "T-050" => {
                // Six years of statements for the free cash flow history, and the dated price.
                for (year, operating_cash_flow) in [
                    (2025, "52600000000"),
                    (2024, "47300000000"),
                    (2023, "41900000000"),
                    (2022, "24600000000"),
                    (2021, "35800000000"),
                    (2020, "33000000000"),
                ] {
                    cases.push(call(
                        "dart_financials",
                        "dart.financials",
                        json!({"company":"99022100","fiscal_year":year,"report_period":"annual","basis":"consolidated"}),
                        &[operating_cash_flow, "operating_cash_flow", "consolidated"],
                    ));
                }
                cases.push(call(
                    "krx_daily_quote",
                    "krx.daily_quote",
                    json!({"symbol":"990221","date":"2026-04-16"}),
                    &["21950", "\"listed_shares\": \"20000000\""],
                ));
                let mut negative = call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2019,"report_period":"annual","basis":"consolidated"}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-052" => {
                // The plan's comparison needs all three companies and Iseul's basis fallback.
                for (name, english, stock, code) in [
                    ("솔숲식품", "Solsup Foods", "990221", "99022100"),
                    ("이슬물류", "Iseul Logistics", "990552", "99055200"),
                    ("한빛화학", "Hanbit Chem", "990777", "99077700"),
                ] {
                    for query in [name, english, stock] {
                        cases.push(call(
                            "dart_company_lookup",
                            "dart.company_lookup",
                            json!({"query":query}),
                            &[code, stock],
                        ));
                    }
                }
                for (company, basis, revenue, income, net_income, receipt, filed) in [
                    (
                        "99022100",
                        "consolidated",
                        "452300000000",
                        "38100000000",
                        "21400000000",
                        "20260318990221",
                        "2026-03-18",
                    ),
                    (
                        "99055200",
                        "separate",
                        "128400000000",
                        "7600000000",
                        "4900000000",
                        "20260320990552",
                        "2026-03-20",
                    ),
                    (
                        "99077700",
                        "consolidated",
                        "286500000000",
                        "31400000000",
                        "19800000000",
                        "20260319990777",
                        "2026-03-19",
                    ),
                ] {
                    cases.push(call(
                        "dart_financials",
                        "dart.financials",
                        json!({"company":company,"fiscal_year":2025,"report_period":"annual","basis":basis}),
                        &[revenue, income, net_income, basis, receipt, filed],
                    ));
                }
                let mut no_data = call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99055200","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["no matching data"],
                );
                no_data.0["error"] = json!("no matching data");
                cases.push(no_data);
                let mut negative = call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2024,"report_period":"annual","basis":"consolidated"}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-053" => {
                // The dividend review reads FY2021 as well as the later comparative tables.
                for query in ["솔숲식품", "Solsup Foods", "990221"] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query}),
                        &["99022100", "990221"],
                    ));
                }
                for (year, receipt, dps, dividends, earnings, payout, cash_yield) in [
                    (
                        2025,
                        "20260318990221",
                        "300",
                        "6000",
                        "21000",
                        "28.6",
                        "1.4",
                    ),
                    (
                        2024,
                        "20250318990221",
                        "250",
                        "5000",
                        "19500",
                        "25.6",
                        "1.3",
                    ),
                    (
                        2023,
                        "20240319990221",
                        "250",
                        "5000",
                        "16900",
                        "29.6",
                        "1.5",
                    ),
                    (2022, "20230320990221", "200", "4000", "9400", "42.6", "1.6"),
                    (
                        2021,
                        "20220321990221",
                        "250",
                        "5000",
                        "17800",
                        "28.1",
                        "1.5",
                    ),
                ] {
                    let mut expected = vec![
                        "dps_cash".to_owned(),
                        "total_cash_dividends".to_owned(),
                        "payout_ratio".to_owned(),
                        "consolidated".to_owned(),
                        receipt.to_owned(),
                        format!("\"fiscal_year\": {year}"),
                    ];
                    for value in [dps, dividends, earnings, payout, cash_yield] {
                        expected.push(format!("\"value\": \"{value}\""));
                    }
                    if year == 2025 {
                        expected.extend([
                            "\"status\": \"found\"".into(),
                            "2025-12-31".into(),
                            "20260205990221".into(),
                        ]);
                    } else {
                        expected.push("\"status\": \"no_notice\"".into());
                    }
                    if year == 2021 {
                        expected.extend([
                            "\"fiscal_year\": 2020".into(),
                            "\"value\": \"16500\"".into(),
                            "\"value\": \"30.3\"".into(),
                        ]);
                    }
                    cases.push(call(
                        "dart_dividends",
                        "dart.dividends",
                        json!({"company":"99022100","year":year}),
                        &expected.iter().map(String::as_str).collect::<Vec<_>>(),
                    ));
                }
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["452300000000", "21000000000", "52600000000", "-18900000000", "consolidated", "20260318990221"],
                ));
                cases.push(call(
                    "dart_filing_read",
                    "dart.filing_read",
                    json!({"rcept_no":"20260205990221"}),
                    &["KRW 300", "2025-12-31", "6,000,000,000"],
                ));
                let mut negative = call(
                    "dart_dividends",
                    "dart.dividends",
                    json!({"company":"99022100","year":2020}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            "T-055" => {
                // These retained statement values are the calculation chip's pinned inputs.
                for query in ["솔숲식품", "Solsup Foods", "990221"] {
                    cases.push(call(
                        "dart_company_lookup",
                        "dart.company_lookup",
                        json!({"query":query}),
                        &["99022100", "990221"],
                    ));
                }
                cases.push(call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"consolidated"}),
                    &["dataset_ref", "revenue", "452300000000", "operating_income", "38100000000", "21400000000", "consolidated", "20260318990221", "2026-03-18"],
                ));
                let mut negative = call(
                    "dart_financials",
                    "dart.financials",
                    json!({"company":"99022100","fiscal_year":2025,"report_period":"annual","basis":"separate"}),
                    &["eval_replay_unmatched"],
                );
                negative.0["error"] = json!("eval_replay_unmatched");
                cases.push(negative);
            }
            _ => panic!("uncovered replay scenario {id}"),
        }
        for (index, (case, expected)) in cases.into_iter().enumerate() {
            fs::write(
                root.path().join("replay-case.json"),
                serde_json::to_vec(&case).unwrap(),
            )
            .unwrap();
            let chat = new_chat(&state, &loupist.id, &format!("{id}-{index}")).unwrap();
            let session = Session {
                id: "replay-case".into(),
                role: "probe".into(),
                memory: "off".into(),
                turns: vec![],
            };
            let result = run_turn(
                &state,
                &chat,
                &session,
                &Turn {
                    user: "REPLAY_CASE_EVAL".into(),
                    as_of: as_of.clone(),
                    ..Default::default()
                },
                "fixture",
                "medium",
                60000,
            )
            .await
            .unwrap();
            assert_eq!(result["status"], "completed", "{id} {case}: {result}");
            let answer = result["answer"].as_str().unwrap();
            for value in expected {
                assert!(
                    answer.contains(&value),
                    "{id} {case}: missing {value} in {answer}"
                );
            }
            for value in case["absent"].as_array().into_iter().flatten() {
                let value = value.as_str().unwrap();
                assert!(
                    !answer.contains(value),
                    "{id} {case}: {value} leaked past the as-of cutoff in {answer}"
                );
            }
            assert!(
                result["tool_calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["name"] == case["name"]),
                "{id}: {result}"
            );
        }
        state.shutdown().await.unwrap();
    }
}
