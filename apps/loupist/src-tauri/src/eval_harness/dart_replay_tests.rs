//! Replays exercise the real DART profile read and statement normalization.
use super::*;

#[tokio::test]
async fn every_dart_statement_replay_returns_dated_facts() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/agent/suites/replay");
    let pacer = || Box::pin(async {}) as futures_util::future::BoxFuture<'static, ()>;
    let mut checked = 0;
    for file in fs::read_dir(directory).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let replay: replay::Replay = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let requests = replay
            .requests
            .iter()
            .filter_map(|entry| {
                let url = reqwest::Url::parse(entry.request.get("url")?.as_str()?).ok()?;
                (url.path() == "/api/fnlttSinglAcntAll.json"
                    && entry.response.body.as_ref()?["status"] == "000")
                    .then(|| url.query_pairs().into_owned().collect::<BTreeMap<_, _>>())
            })
            .collect::<Vec<_>>();
        if requests.is_empty() {
            continue;
        }
        let _guard = replay.install("dated-dart-fixture").unwrap();
        // A new service per fixture prevents its profile cache from hiding a
        // missing company.json in another fixture for the same company.
        let service = crate::finance_data::FinanceDataService::new().unwrap();
        for query in requests {
            let year: i32 = query["bsns_year"].parse().unwrap();
            let period = match query["reprt_code"].as_str() {
                "11011" => "annual",
                "11013" => "q1",
                "11012" => "half_year",
                "11014" => "q3",
                code => panic!("unknown fixture report code {code}"),
            };
            let result = service.opendart_company_facts(json!({
                "corp_code": query["corp_code"], "fiscal_year": year, "report_period": period,
                "basis": if query["fs_div"] == "CFS" { "consolidated" } else { "separate" },
            }), "eval-placeholder", &pacer).await.unwrap();
            let facts = result["data"]["facts"].as_array().unwrap();
            assert!(!facts.is_empty(), "{}", path.display());
            for fact in facts {
                assert_eq!(
                    fact["period_end"],
                    format!("{year}-12-31"),
                    "{}: {fact}",
                    path.display()
                );
                if fact["period_kind"] != "instant" {
                    assert_eq!(
                        fact["period_start"],
                        format!("{year}-01-01"),
                        "{}: {fact}",
                        path.display()
                    );
                }
            }
            assert!(
                result["quality"]["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|check| { check["code"] == "period_dates" && check["status"] == "pass" }),
                "{}: {result}",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(checked > 0);
}
