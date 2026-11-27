//! Scripted World publications of the change-feed scenarios run through the
//! production pipeline with no provider: endings, conflicts and Lens marks
//! come from the publication transaction, and no World model request is made.
use super::*;
use crate::knowledge::{GraphClaim, CONFLICTS_WITH_CLAIM, END_BASIS, SUPERSEDED_BY_CLAIM};

const SUITE: &str = include_str!("../../../evals/agent/suites/change-feed-v1.json");

/// A change-feed scenario as the repository suite states it.
fn suite_scenario(id: &str) -> Value {
    serde_json::from_str::<Value>(SUITE).unwrap()["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("change-feed scenario {id}"))
}

fn scenario(value: Value) -> Scenario {
    serde_json::from_value(value).unwrap()
}

/// CF-002 with the CFO claims contradicting but not the same assertion: the
/// two stay in conflict instead of one ending the other.
fn cf_002_in_conflict() -> Scenario {
    let mut value = suite_scenario("CF-002");
    for publication in value["world"]["publications"].as_array_mut().unwrap() {
        if let Some(resolution) = publication["script"]["resolutions"].get_mut("q3-cfo") {
            resolution["same_assertion"] = json!([]);
        }
    }
    scenario(value)
}

struct Staged {
    _root: tempfile::TempDir,
    state: AppState,
    prepared: trial::Prepared,
}

impl Staged {
    async fn new(scenario: &Scenario) -> Self {
        validate_seed(scenario).unwrap();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let state = trial::trial_state(root.path().join("app"));
        let model = state.model_catalog().unwrap().resolve("fixture").unwrap();
        let prepared = trial::prepare(&state, &workspace, scenario, &model, "medium", 120_000)
            .await
            .unwrap();
        Self {
            _root: root,
            state,
            prepared,
        }
    }

    async fn after_turn(&mut self, session: &str, turn: usize) {
        let staging = self.prepared.staging.as_mut().unwrap();
        staging
            .publish(
                &self.state,
                &self.prepared.loupist_id,
                &self.prepared.retained,
                &world::script::After::turn(session, turn),
                120_000,
            )
            .await
            .unwrap();
    }

    fn store(&self) -> KnowledgeStore {
        KnowledgeStore::for_loupist(self.state.store.clone(), &self.prepared.loupist_id).unwrap()
    }

    fn claim(&self, fact: &str) -> GraphClaim {
        let id = self
            .prepared
            .staging
            .as_ref()
            .unwrap()
            .claim(fact)
            .unwrap()
            .to_owned();
        world::script::head(&self.store(), &self.prepared.loupist_id)
            .unwrap()
            .unwrap()
            .claims
            .into_iter()
            .find(|claim| claim.id == id)
            .unwrap()
    }

    fn trace(&self) -> Value {
        self.prepared
            .staging
            .as_ref()
            .unwrap()
            .trace(&self.state, &self.prepared.loupist_id)
            .unwrap()
    }

    /// The World model requests the profile recorded: none, when scripted.
    fn world_requests(&self) -> usize {
        let now = crate::commands::now_ms();
        self.state
            .store
            .model_usage_in_range(0, now + 86_400_000)
            .unwrap()
            .into_iter()
            .filter(|row| {
                matches!(
                    row.data.surface,
                    crate::runs::log::UsageSurface::World
                        | crate::runs::log::UsageSurface::WorldSchema
                )
            })
            .count()
    }

    async fn finish(self) {
        self.state.shutdown().await.unwrap();
    }
}

fn scripted_answers(trace: &Value) {
    for publication in trace["publications"].as_array().unwrap() {
        assert_eq!(publication["status"], "completed", "{publication}");
        assert_eq!(publication["unresolved"], json!([]), "{publication}");
        assert!(
            publication["stage_answers"]["ExtractedEdges"].as_u64() >= Some(1),
            "{publication}"
        );
    }
}

#[tokio::test]
async fn cf_001_ends_the_cfo_claim_and_marks_the_section_it_grounds_superseded() {
    let mut staged = Staged::new(&scenario(suite_scenario("CF-001"))).await;
    let page = staged.prepared.lens_pages[0]["record_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let section_state = |staged: &Staged| {
        let page = staged
            .store()
            .read(|read| read.read_page(&staged.prepared.loupist_id, &page))
            .unwrap();
        // The page view states a Superseded section by when and by which claims.
        let section = serde_json::to_value(&page.body).unwrap()["sections"][0].clone();
        let kind = if section["contested"] == true {
            "contested"
        } else if section["claimIds"].is_array() && section["sinceMs"].is_number() {
            "superseded"
        } else {
            "current"
        };
        json!({"kind": kind, "section": section})
    };
    assert_eq!(staged.trace()["publications"].as_array().unwrap().len(), 1);
    assert!(staged.claim("q2-cfo").invalid_at.is_none());
    assert_eq!(
        section_state(&staged)["kind"],
        "current",
        "{}",
        section_state(&staged)
    );
    // The Q3 report and the commissioning notice are queued at seed and
    // stay pending until their publication after the first turn.
    let pending = staged
        .store()
        .graph_jobs(&staged.prepared.loupist_id)
        .unwrap()
        .into_iter()
        .filter(|job| job.status == "queued")
        .count();
    assert_eq!(pending, 2);

    staged.after_turn("bot", 0).await;
    let trace = staged.trace();
    scripted_answers(&trace);
    assert_eq!(
        trace["publications"].as_array().unwrap().len(),
        3,
        "{trace}"
    );
    let ended = staged.claim("q2-cfo");
    let replacing = staged
        .prepared
        .staging
        .as_ref()
        .unwrap()
        .claim("q3-cfo")
        .unwrap()
        .to_owned();
    assert!(ended.invalid_at.is_some(), "{ended:?}");
    assert_eq!(
        ended.qualifier(SUPERSEDED_BY_CLAIM),
        Some(replacing.as_str())
    );
    assert_eq!(ended.qualifier(END_BASIS), Some("reference"));
    assert_eq!(
        section_state(&staged)["kind"],
        "superseded",
        "{}",
        section_state(&staged)
    );
    // The trace's Lens state says so too.
    let chat = new_chat(&staged.state, &staged.prepared.loupist_id, "probe").unwrap();
    let lens = trace::lens_state(&staged.state, &chat, None).unwrap();
    let section = &lens["pages"][0]["sections"][0]["state"];
    assert_eq!(section["kind"], "superseded", "{lens}");
    assert_eq!(section["claim_ids"], json!([ended.id]), "{lens}");
    assert!(trace["ended"].as_u64() >= Some(1), "{trace}");
    assert_eq!(staged.world_requests(), 0);
    staged.finish().await;
}

#[tokio::test]
async fn cf_002_keeps_contradicting_claims_in_conflict_and_ends_the_supply_on_its_stated_day() {
    let staged = Staged::new(&cf_002_in_conflict()).await;
    let trace = staged.trace();
    scripted_answers(&trace);
    let (old, new) = (staged.claim("q2-cfo"), staged.claim("q3-cfo"));
    assert!(old.invalid_at.is_none() && new.invalid_at.is_none());
    assert_eq!(old.qualifier(CONFLICTS_WITH_CLAIM), Some(new.id.as_str()));
    assert_eq!(new.qualifier(CONFLICTS_WITH_CLAIM), Some(old.id.as_str()));
    let supply = staged.claim("q2-supply");
    assert_eq!(
        supply
            .invalid_at
            .as_ref()
            .and_then(|end| end.value.as_deref()),
        Some("2026-09-23"),
        "{supply:?}"
    );
    assert_eq!(supply.qualifier(END_BASIS), Some("stated"));
    assert!(trace["conflicts"].as_u64() >= Some(2), "{trace}");
    assert_eq!(staged.world_requests(), 0);
    staged.finish().await;
}

#[tokio::test]
async fn cf_003_hides_an_ending_published_after_the_cutoff() {
    use crate::commands::knowledge::as_of::{world_claims_as_of, Cutoff};
    let staged = Staged::new(&scenario(suite_scenario("CF-003"))).await;
    scripted_answers(&staged.trace());
    let supply = staged.claim("q2-supply").id;
    let store = staged.store();
    let owner = staged.prepared.loupist_id.clone();
    let head = store
        .list_records(&owner)
        .unwrap()
        .into_iter()
        .find(|record| record.kind == RecordKind::World)
        .and_then(|record| record.revision_id)
        .unwrap();
    let at = |day: &str| {
        let cutoff = Cutoff(chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap());
        store
            .transact(|write| world_claims_as_of(write, &owner, &head, cutoff))
            .unwrap()
            .remove(&supply)
    };
    // A seeded Source is its text, whatever document metadata the seed states.
    let retained = &staged.prepared.retained["alderquay-q2"];
    let span = store
        .read_span(ReadSpan {
            loupist_id: owner.clone(),
            record_id: retained.record_id.clone(),
            span_id: retained.span_id.clone().unwrap(),
            at_ms: crate::commands::now_ms(),
        })
        .unwrap();
    assert_eq!(span.media_type, "text/plain");
    assert_eq!(
        std::str::from_utf8(&span.bytes).unwrap(),
        suite_scenario("CF-003")["seed"]["sources"][0]["text"]
    );
    let before = at("2026-10-01").expect("Q2's supply claim is known by October 1");
    assert!(before.invalid_at.is_none(), "{before:?}");
    let after = at("2026-10-23").unwrap();
    assert!(after.invalid_at.is_some(), "{after:?}");
    assert_eq!(staged.world_requests(), 0);
    staged.finish().await;
}

#[tokio::test]
async fn an_overridden_fact_policy_opens_every_world_stage() {
    const CHILD: &str = "LOUPIST_TEST_WORLD_PROMPTS_CHILD";
    const POLICY: &str = "Evaluation fact policy: state only what the Source says.";
    if std::env::var_os(CHILD).is_none() {
        // The override is read once per process, so a child holds it alone.
        let prompts = tempfile::tempdir().unwrap();
        let bundled =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/knowledge/world_extract/prompts");
        for entry in fs::read_dir(&bundled).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "txt") {
                fs::copy(&path, prompts.path().join(path.file_name().unwrap())).unwrap();
            }
        }
        fs::write(prompts.path().join("policy.txt"), POLICY).unwrap();
        super::tests::run_child_test(
            module_path!(),
            "an_overridden_fact_policy_opens_every_world_stage",
            |child| {
                child
                    .env(CHILD, "1")
                    .env("LOUPIST_EVAL_WORLD_PROMPTS", prompts.path())
            },
        );
        return;
    }
    assert!(crate::knowledge::world_extract::world_prompts_sha256()
        .unwrap()
        .is_some());
    let staged = Staged::new(&scenario(suite_scenario("CF-002"))).await;
    let systems = &staged.prepared.staging.as_ref().unwrap().systems;
    assert!(!systems.is_empty());
    for system in systems {
        assert!(system.starts_with(POLICY), "{system}");
    }
    staged.finish().await;
}

#[tokio::test]
async fn an_asset_link_names_its_followed_asset_s_world_company() {
    let mut linked = suite_scenario("CF-002");
    linked["seed"]["watchlist"] = json!([{"name": "Alderquay Bearings"}]);
    linked["seed"]["asset_links"] =
        json!([{"asset": "Alderquay Bearings", "company": "Alderquay Bearings"}]);
    let staged = Staged::new(&scenario(linked)).await;
    let owner = staged.prepared.loupist_id.clone();
    let company = world::script::head(&staged.store(), &owner)
        .unwrap()
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.name == "Alderquay Bearings")
        .unwrap()
        .id;
    let asset = staged
        .state
        .store
        .assets_list(&owner, true)
        .unwrap()
        .remove(0)
        .asset_id;
    let linked: Option<String> = staged
        .state
        .store
        .lock_loupist(&owner)
        .unwrap()
        .query_row(
            "SELECT entity_id FROM knowledge_asset_pages WHERE loupist_id=?1 AND asset_id=?2",
            rusqlite::params![owner, asset],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked.as_deref(), Some(company.as_str()));
    staged.finish().await;
}
