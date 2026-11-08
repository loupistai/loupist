//! Paid live check that method learning reviews only research filed in two Chats
//! on two Sources, and that read-only Chats write nothing. Whether a useful
//! method was learned and reused is recorded in `measures`, not asserted.
use super::*;
use crate::knowledge::lens_page::{BasisRef, LensKind, LensOp};
use rusqlite::OptionalExtension;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema: String,
    loupist: Profile,
    training: Vec<Step>,
    probes: Probes,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    name: String,
    focus: String,
    instructions: String,
}
#[derive(Deserialize)]
struct Step {
    id: String,
    #[serde(flatten)]
    turn: Turn,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probes {
    applicable: Step,
    unrelated: Step,
}

struct Run {
    fixture: Fixture,
    root: PathBuf,
    pi: PathBuf,
    frozen: artifacts::FrozenArtifacts,
    model: String,
    thinking: String,
}

struct Arm {
    state: AppState,
    inference: crate::providers::inference::ProviderInferenceService,
    loupist: String,
}

/// One JSON document, replaced atomically after every turn so a failure keeps what ran.
struct Report {
    out: PathBuf,
    doc: Value,
}
impl Report {
    fn save(&self) -> Result<()> {
        let parent = self
            .out
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, &self.doc)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.out)?;
        Ok(())
    }
}

async fn open_arm(run: &Run, name: &str) -> Result<Arm> {
    // Short directory names keep the broker socket under the 103-byte Unix limit.
    let dir = run.root.join(&name[..3]);
    let workspace = dir.join("Industrials");
    fs::create_dir_all(&workspace)?;
    let mut state = AppState::for_eval(dir.join("app"));
    fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        format!(
            "[appearance]\nlanguage = 'en'\n{}",
            run.frozen.config_toml()
        ),
    )?;
    state.eval_pi_agent_dir = Some(run.pi.clone());
    Arc::make_mut(&mut state.artifacts).pi = Some(run.frozen.pi.clone());
    let provider = run
        .model
        .split_once('/')
        .ok_or("LOUPIST_RESEARCH_MODEL must be provider/model")?
        .0;
    let model = crate::providers::connection::eval_models(&state, provider)
        .await?
        .into_iter()
        .find(|m| m.id == run.model)
        .ok_or("requested research model unavailable")?;
    model.validate_thinking_level(&run.thinking)?;
    state.set_model_catalog(crate::providers::catalog::ModelCatalog {
        models: vec![model.clone()],
    })?;
    let inference = crate::providers::inference::ProviderInferenceService::from_state(&state)?;
    state.graph.shutdown().await?;
    state.graph = GraphSupervisor::with_inference(
        state.store.clone(),
        state.artifacts.app_data_dir.clone(),
        Some(staged_resources("embed_assets")?),
        state.maintenance.clone(),
        Some(inference.clone()),
    );
    if !state.graph.runtime_available() {
        return Err("Graph runtime unavailable".into());
    }
    let profile = &run.fixture.loupist;
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
    state.store.update_loupist_profile(
        &loupist.id,
        crate::store::LoupistProfileEdit {
            name: Some(&profile.name),
            focus: Some(&profile.focus),
            instructions: Some(&profile.instructions),
            ..Default::default()
        },
        crate::commands::now_ms(),
    )?;
    enable_connections(&state, &loupist.id, &[])?;
    let config = GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some(model.provider.clone()),
        helper_model: Some(model.model.clone()),
        helper_model_profile_id: Some(model.id.clone()),
        thinking_level: run.thinking.clone(),
        timeout_ms: 120_000,
        run_options: model
            .run_controls
            .iter()
            .map(|c| (c.id.clone(), c.default_choice.clone()))
            .collect(),
        ..Default::default()
    };
    {
        let mut conn = state.store.lock_loupist(&loupist.id)?;
        let tx = conn.transaction()?;
        store_settings(&tx, &loupist.id, &config)?;
        learning::save_settings(&tx, &loupist.id, true)?;
        tx.commit()?;
    }
    Ok(Arm {
        state,
        inference,
        loupist: loupist.id,
    })
}

async fn ask(run: &Run, arm: &Arm, chat: &ChatSession, memory: &str, step: &Step) -> Result<Value> {
    eprintln!("research: {} ({memory}) in {}", step.id, chat.title);
    let session = Session {
        id: step.id.clone(),
        role: String::new(),
        memory: memory.into(),
        turns: vec![],
    };
    let mut turn = run_turn(
        &arm.state,
        chat,
        &session,
        &step.turn,
        &run.model,
        &run.thinking,
        600_000,
    )
    .await?;
    turn["step"] = json!(step.id);
    turn["chat_title"] = json!(chat.title);
    Ok(turn)
}

fn reviews(arm: &Arm) -> Result<Vec<Value>> {
    let conn = arm.state.store.lock_loupist(&arm.loupist)?;
    let mut q = conn.prepare(
        "SELECT review_id,status,reason,evidence_count,rationale,lens_record_id,through_event
         FROM knowledge_learning_reviews WHERE loupist_id=?1 ORDER BY created_at_ms,review_id",
    )?;
    let rows = q
        .query_map([&arm.loupist], |r| {
            Ok(json!({"review_id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,
                "reason":r.get::<_,Option<String>>(2)?,"evidence_count":r.get::<_,i64>(3)?,
                "rationale":r.get::<_,Option<String>>(4)?,"lens_record_id":r.get::<_,Option<String>>(5)?,
                "through_event":r.get::<_,i64>(6)?}))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Turn-filed sections that cite Source spans, read from the Lens log rather than
/// the learning gate, so a missing review can be explained by what was filed.
/// What identity learning had to work with: each subject anchor, the World's
/// nodes and how its claims cite evidence, so a missing review can be traced.
fn identity(arm: &Arm) -> Result<Value> {
    let conn = arm.state.store.lock_loupist(&arm.loupist)?;
    let mut q = conn.prepare(
        "SELECT a.record_id, m.title, a.role, a.node_id, a.name_fold FROM knowledge_lens_anchors a
         JOIN knowledge_lens_metadata m ON m.record_id = a.record_id WHERE a.loupist_id = ?1 ORDER BY a.rowid",
    )?;
    let anchors = q
        .query_map([&arm.loupist], |r| {
            Ok(
                json!({"page": r.get::<_, String>(1)?, "role": r.get::<_, String>(2)?,
                "node_id": r.get::<_, Option<String>>(3)?, "name_fold": r.get::<_, String>(4)?}),
            )
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut q = conn.prepare(
        "SELECT l.op_json, m.title FROM knowledge_lens_log l JOIN knowledge_lens_metadata m ON m.record_id = l.record_id
         WHERE l.loupist_id = ?1 AND l.origin = 'turn' AND l.outcome IN ('added','replaced') ORDER BY l.rowid",
    )?;
    let mut cited = BTreeMap::<String, BTreeSet<String>>::new();
    for row in q.query_map([&arm.loupist], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (op, title) = row?;
        if let Ok(LensOp::Put { basis, .. }) = serde_json::from_str::<LensOp>(&op) {
            for r in basis.refs {
                if let BasisRef::Span {
                    record_id, span_id, ..
                } = r
                {
                    cited
                        .entry(title.clone())
                        .or_default()
                        .insert(format!("{record_id}/{span_id}"));
                }
            }
        }
    }
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT s.snapshot_json FROM knowledge_world_snapshots s JOIN knowledge_record_heads h ON h.revision_id = s.revision_id
             JOIN knowledge_records r ON r.record_id = h.record_id WHERE r.owner_loupist_id = ?1 AND r.kind = 'world'",
            [&arm.loupist],
            |r| r.get(0),
        )
        .optional()?;
    let world: Value = snapshot
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or(Value::Null);
    let nodes: Vec<Value> = world["nodes"].as_array().cloned().unwrap_or_default().iter()
        .map(|n| json!({"id": n["id"], "name": n["name"], "aliases": n["aliases"], "kind": n["kind"]}))
        .collect();
    let claims: Vec<Value> = world["claims"].as_array().cloned().unwrap_or_default().iter().take(12)
        .map(|c| json!({"status": c["status"], "endpoints": c["endpoints"], "evidence": c["evidence"]}))
        .collect();
    Ok(
        json!({"anchors": anchors, "cited_spans": cited, "nodes": nodes,
        "claim_count": world["claims"].as_array().map_or(0, Vec::len), "claims_sample": claims}),
    )
}

fn filed(arm: &Arm) -> Result<Value> {
    let store = KnowledgeStore::for_loupist(arm.state.store.clone(), &arm.loupist)?;
    let titles: BTreeMap<_, _> = store
        .list_records(&arm.loupist)?
        .into_iter()
        .map(|r| (r.record_id, r.title))
        .collect();
    let conn = arm.state.store.lock_loupist(&arm.loupist)?;
    let mut q = conn.prepare(
        "SELECT l.op_json,l.chat_id,l.message_id,l.record_id,m.kind FROM knowledge_lens_log l
         JOIN knowledge_lens_metadata m ON m.record_id=l.record_id
         WHERE l.loupist_id=?1 AND l.origin='turn' AND l.outcome IN ('added','replaced')
         ORDER BY l.rowid",
    )?;
    let rows = q
        .query_map([&arm.loupist], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let (mut turns, mut chats, mut sources) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut sections = vec![];
    for (op, chat, message, page, kind) in rows {
        let LensOp::Put { key, basis, .. } = serde_json::from_str(&op)? else {
            continue;
        };
        let cited: BTreeSet<_> = basis
            .refs
            .iter()
            .filter_map(|r| match r {
                BasisRef::Span { record_id, .. } => Some(record_id.clone()),
                _ => None,
            })
            .collect();
        if cited.is_empty() {
            continue;
        }
        turns.insert(message.clone());
        chats.insert(chat.clone());
        sources.extend(cited.iter().cloned());
        sections.push(json!({"chat_id":chat,"message_id":message,"page":titles.get(&page),"kind":kind,
            "key":key.as_str(),"sources":cited.iter().map(|id| titles.get(id).unwrap_or(id)).collect::<Vec<_>>()}));
    }
    Ok(json!({"turns":turns.len(),"chats":chats.len(),"sources":sources.len(),"sections":sections}))
}

fn methods(arm: &Arm) -> Result<Vec<Value>> {
    let store = KnowledgeStore::for_loupist(arm.state.store.clone(), &arm.loupist)?;
    let records = store.list_records(&arm.loupist)?;
    let mut out = vec![];
    for record in records.iter().filter(|r| r.kind == RecordKind::Lens) {
        let page = store.read(|read| read.read_page(&arm.loupist, &record.record_id))?;
        if !page.learned || page.kind != LensKind::Method {
            continue;
        }
        let body = serde_json::to_value(&page.body)?;
        let sections = body["sections"].as_array().cloned().unwrap_or_default().into_iter().map(|s| {
            let refs = s["basis"]["refs"].as_array().cloned().unwrap_or_default();
            let spans: BTreeSet<_> = refs.iter().filter(|r| r["kind"] == "span")
                .filter_map(|r| r["recordId"].as_str().map(str::to_owned)).collect();
            json!({"key":s["key"],"heading":s["heading"],"markdown":s["markdown"],"origin":s["origin"],
                "basis":refs.iter().filter_map(|r| r["kind"].as_str()).collect::<Vec<_>>(),
                "span_sources":spans.iter().map(|id| records.iter().find(|r| &r.record_id == id).map_or(id.as_str(), |r| r.title.as_str())).collect::<Vec<_>>(),
                "span_source_ids":spans})
        }).collect::<Vec<_>>();
        out.push(
            json!({"record_id":page.record_id,"title":page.title,"summary":body["summary"],
            "anchors":page.anchors,"sections":sections}),
        );
    }
    Ok(out)
}

/// Runs each group of training turns in its own Chat, draining World and learning after every turn.
async fn train(
    run: &Run,
    arm: &Arm,
    chats: &[&[usize]],
    report: &mut Report,
    key: &str,
) -> Result<bool> {
    for (n, group) in chats.iter().enumerate() {
        let chat = new_chat(&arm.state, &arm.loupist, &format!("Research {}", n + 1))?;
        for &index in *group {
            let mut turn = ask(run, arm, &chat, "remember", &run.fixture.training[index]).await?;
            turn["drain"] = growth::drain(&arm.state, &arm.loupist).await?;
            let completed = turn["status"] == "completed";
            report.doc["arms"][key]["turns"]
                .as_array_mut()
                .ok_or("arm turns missing")?
                .push(turn);
            report.save()?;
            if !completed {
                return Ok(false);
            }
        }
    }
    let arm_doc = &mut report.doc["arms"][key];
    arm_doc["reviews"] = json!(reviews(arm)?);
    arm_doc["filed_evidence"] = filed(arm)?;
    arm_doc["identity"] = identity(arm)?;
    arm_doc["methods"] = json!(methods(arm)?);
    arm_doc["helper_inference"] = json!(arm.inference.eval_observations());
    report.save()?;
    Ok(true)
}

fn method_reads(turn: &Value, ids: &[String]) -> Vec<Value> {
    turn["memory_reads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["access"] == "exact_read" && ids.iter().any(|id| r["record_id"] == *id))
        .cloned()
        .collect()
}

async fn probe(run: &Run, arm: &Arm, report: &mut Report) -> Result<()> {
    let ids: Vec<String> = report.doc["arms"]["two-chats"]["methods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["record_id"].as_str().map(str::to_owned))
        .collect();
    report.doc["arms"]["two-chats"]["reviews_before_probes"] = json!(reviews(arm)?.len());
    for (key, step) in [
        ("p1", &run.fixture.probes.applicable),
        ("p2", &run.fixture.probes.unrelated),
    ] {
        let chat = new_chat(&arm.state, &arm.loupist, &step.id)?;
        let mut turn = ask(run, arm, &chat, "read_only", step).await?;
        turn["method_reads"] = json!(method_reads(&turn, &ids));
        turn["lens_reads"] = json!(turn["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["name"] == "lens_read")
            .map(
                |c| json!({"record_id":c["args"]["recordId"],"status":c["args"]["status"],
                "learned_method":ids.iter().any(|id| c["args"]["recordId"] == *id)})
            )
            .collect::<Vec<_>>());
        turn["reviews_after"] = json!(reviews(arm)?.len());
        report.doc["arms"]["two-chats"]["probes"][key] = turn;
        report.save()?;
    }
    Ok(())
}

/// Model-dependent outcomes, derived only from the recorded document so a reader can recompute them.
fn measures(doc: &Value) -> Value {
    let two = &doc["arms"]["two-chats"];
    let reviews = two["reviews"].as_array().cloned().unwrap_or_default();
    let review = (reviews.len() == 1).then(|| &reviews[0]);
    let method = two["methods"].as_array().and_then(|m| m.first());
    let learned = |m: &Value| -> Vec<Value> {
        m["sections"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["origin"] == "learning")
            .cloned()
            .collect()
    };
    let p1 = &two["probes"]["p1"];
    let p2 = &two["probes"]["p2"];
    let read =
        |p: &Value| method.map(|_| p["method_reads"].as_array().is_some_and(|r| !r.is_empty()));
    let wrote_nothing = |p: &Value| {
        p["status"] == "completed"
            && p["memory_commits"].as_array().is_some_and(Vec::is_empty)
            && p["lens"]["pages"].as_array().is_some_and(|pages| {
                pages
                    .iter()
                    .all(|page| page["ops_this_turn"].as_array().is_some_and(Vec::is_empty))
            })
            && p["reviews_after"] == two["reviews_before_probes"]
    };
    let method_created = method.is_some()
        && review.is_some_and(|r| r["status"] == "created" || r["status"] == "refined");
    let spans_sources = method.map(|m| {
        learned(m)
            .iter()
            .map(|s| s["span_source_ids"].as_array().map_or(0, Vec::len))
            .min()
            .unwrap_or(0)
    });
    let limits = method.and_then(|m| {
        m["sections"]
            .as_array()?
            .iter()
            .find(|s| s["key"] == "limits")
            .map(|s| s["markdown"].clone())
    });
    let applied = method.map(|_| {
        p1["lens_reads"].as_array().is_some_and(|r| {
            r.iter()
                .any(|c| c["learned_method"] == true && c["status"] == "applied")
        })
    });
    let probes_clean = !p1.is_null() && !p2.is_null() && wrote_nothing(p1) && wrote_nothing(p2);
    json!({
        "review_status": review.map(|r| r["status"].clone()),
        "method_created": method_created,
        "method_spans_sources": spans_sources,
        "method_limits": limits,
        "p1_method_read": read(p1),
        "p1_applied": applied,
        "p2_method_read": read(p2),
        "probes_wrote_nothing": probes_clean,
        "one_chat_reviews": doc["arms"]["one-chat"]["reviews"].as_array().map(Vec::len),
    })
}

/// Deterministic invariants only; model quality stays in `measures`.
fn failures(doc: &Value) -> Vec<String> {
    let mut out = vec![];
    for (key, arm) in doc["arms"].as_object().into_iter().flatten() {
        if arm["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|t| t["status"] != "completed")
        {
            out.push(format!("{key}: a training turn did not complete"));
        }
    }
    let m = &doc["measures"];
    if m["one_chat_reviews"] != 0 {
        out.push(format!(
            "one-chat: expected no learning review, found {}",
            m["one_chat_reviews"]
        ));
    }
    let two = &doc["arms"]["two-chats"];
    let reviews = two["reviews"].as_array().map_or(0, Vec::len);
    let terminal = ["created", "refined", "deferred", "failed"];
    if reviews != 1 || !terminal.iter().any(|s| m["review_status"] == *s) {
        out.push(format!(
            "two-chats: expected one settled review, found {reviews} (status {}); filed evidence: {} turns, {} chats, {} sources",
            m["review_status"], two["filed_evidence"]["turns"], two["filed_evidence"]["chats"], two["filed_evidence"]["sources"]
        ));
    }
    if m["probes_wrote_nothing"] != true {
        out.push("probes: a read-only probe failed, committed Memory, filed Lens ops or started a review".into());
    }
    if m["method_spans_sources"].as_u64().is_some_and(|n| n < 2) {
        out.push(
            "method: a learned section's basis cites spans from fewer than two Sources".into(),
        );
    }
    out
}

fn git(args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "Paid live method-learning proof; requires a Pi directory, model and new output path"]
async fn live_lens_research_v1() -> Result<()> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bytes = fs::read(manifest.join("../evals/lens/research-v1.json"))?;
    let fixture: Fixture = serde_json::from_slice(&bytes)?;
    if fixture.schema != "loupist-lens-research/1"
        || fixture.training.len() != 2
        || fixture
            .training
            .iter()
            .any(|s| s.turn.attachments.len() != 1)
        || fixture.probes.applicable.turn.attachments.len() != 1
        || !fixture.probes.unrelated.turn.attachments.is_empty()
    {
        return Err("research fixture needs two attached originals and two probes".into());
    }
    let out = PathBuf::from(std::env::var("LOUPIST_RESEARCH_OUT")?);
    if out.exists() {
        return Err("refusing to overwrite research evidence".into());
    }
    let pi = PathBuf::from(std::env::var("LOUPIST_RESEARCH_PI_AGENT_DIR")?);
    if !pi.is_absolute() || !pi.is_dir() {
        return Err("Pi directory must exist and be absolute".into());
    }
    let model = std::env::var("LOUPIST_RESEARCH_MODEL")?;
    let thinking = std::env::var("LOUPIST_RESEARCH_THINKING").unwrap_or_else(|_| "medium".into());
    // A Chat's broker socket lives under the app directory, and Unix sockets allow
    // 103 path bytes; macOS `temp_dir()` is a long /var/folders path.
    let base = if cfg!(target_os = "macos") {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let temp = tempfile::Builder::new().prefix("lr-").tempdir_in(base)?;
    let frozen = artifacts::FrozenArtifacts::new(artifacts()?)?;
    let mut report = Report {
        out,
        doc: json!({"schema":"loupist-lens-research-run/1","fixture_sha256":crate::util::sha256(&bytes),
            "agent_snapshot_sha256":frozen.sha256,"model":model,"thinking":thinking,
            "git_commit":git(&["rev-parse","HEAD"]),"git_dirty":!git(&["status","--porcelain"]).is_empty(),
            "arms":{"two-chats":{"turns":[]},"one-chat":{"turns":[]}}}),
    };
    report.save()?;
    let run = Run {
        fixture,
        root: temp.path().to_path_buf(),
        pi,
        frozen,
        model,
        thinking,
    };

    let arm = open_arm(&run, "two-chats").await?;
    let result = async {
        if train(&run, &arm, &[&[0], &[1]], &mut report, "two-chats").await? {
            probe(&run, &arm, &mut report).await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    arm.state.shutdown().await?;
    result?;

    let arm = open_arm(&run, "one-chat").await?;
    let result = train(&run, &arm, &[&[0, 1]], &mut report, "one-chat").await;
    arm.state.shutdown().await?;
    result?;

    report.doc["measures"] = measures(&report.doc);
    let failed = failures(&report.doc);
    report.doc["failures"] = json!(failed);
    report.save()?;
    eprintln!("research: measures {}", report.doc["measures"]);
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; ").into())
    }
}
