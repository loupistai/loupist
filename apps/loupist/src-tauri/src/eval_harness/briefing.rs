//! Paid opt-in proof of the real unattended briefing dispatch and persisted inbox.
use super::*;

pub async fn run(pi_agent_dir: PathBuf, out: PathBuf) -> Result<()> {
    if !pi_agent_dir.is_absolute() {
        return Err("Pi agent directory must be absolute".into());
    }
    if out.exists() {
        return Err("Refusing to overwrite an existing briefing trace".into());
    }
    let temporary = tempfile::Builder::new()
        .prefix("loupist-briefing-eval-")
        .tempdir_in(if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        })?;
    let workspace = temporary.path().join("workspace");
    fs::create_dir(&workspace)?;
    let mut state = AppState::for_eval(temporary.path().join("app"));
    fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        "[appearance]\nlanguage = 'en'\n[pi.retry]\nenabled = false\nmax_retries = 0\n",
    )?;
    state.eval_pi_agent_dir = Some(pi_agent_dir);
    Arc::make_mut(&mut state.artifacts).pi = Some(artifacts()?);
    let model = crate::providers::connection::eval_models(&state, "openai-codex")
        .await?
        .into_iter()
        .find(|model| model.id == "openai-codex/gpt-6.1-sol")
        .ok_or("Required model openai-codex/gpt-6.1-sol is unavailable")?;
    model.validate_thinking_level("medium")?;
    state.set_model_catalog(crate::providers::catalog::ModelCatalog {
        models: vec![model.clone()],
    })?;
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
    enable_connections(&state, &loupist.id, &[])?;
    for symbol in ["HANU", "MORR", "BLUE"] {
        state.store.watchlist_add(symbol, "NASDAQ", None)?;
    }
    seed(&state,&loupist.id,&Seed {lens:vec![],sources:vec![
        SourceSeed {title:"Hanul Precision Q3 release, 2026-10-27".into(),origin:"https://hanul.example/2026-10-27-q3".into(),text:"Fictional fixture. Published 2026-10-27. HANU / Hanul Precision: the largest customer represented 38% of Q3 revenue, up from 24% in Q2. Q3 revenue grew 11% year over year. No other changes are supplied.".into()},
        SourceSeed {title:"Morrow Components capacity notice, 2026-10-27".into(),origin:"https://morrow.example/2026-10-27-capacity".into(),text:"Fictional fixture. Published 2026-10-27. MORR / Morrow Components added one production shift on October 26. The company did not quantify the extra capacity.".into()},
        SourceSeed {title:"Bluehaven Logistics update, 2026-10-28".into(),origin:"https://bluehaven.example/2026-10-28-update".into(),text:"Fictional fixture. Published 2026-10-28. BLUE / Bluehaven Logistics reported average delivery times of 12 days, down from 14 days in its September report.".into()},
    ],..Default::default()})?;
    {
        let mut connection = state.store.lock_loupist(&loupist.id)?;
        let tx = connection.transaction()?;
        crate::knowledge::principles::principle_save(&tx,&loupist.id,None,None,crate::knowledge::principles::PrincipleInput {
            statement:"My diversification principle requires that no single customer account for more than 25% of a company's revenue.".into(),why:Some("I track customer concentration independently of headline revenue growth.".into()),example:None,origin:"settings".into(),origin_chat_id:None,origin_message_id:None,
        },crate::commands::now_ms())?;
        tx.commit()?;
    }
    let settings = crate::briefings::commands::settings_set(
        &state,
        crate::briefings::commands::SettingsRequest {
            loupist_id: loupist.id.clone(),
            enabled: true,
            schedule: None,
        },
    )?;
    let task = settings.task.ok_or("Briefing task was not created")?;
    let captured = Arc::new(Mutex::new(trace::PiTrace::default()));
    let observer = captured.clone();
    state.scheduled.observe_eval(Arc::new(move |event| {
        observer.lock().unwrap().observe(event)
    }));
    let started = chrono::Utc::now().to_rfc3339();
    let clock = Instant::now();
    let run = state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, true)
        .await?
        .ok_or("Briefing was not admitted")?;
    let result = tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            let current = state
                .store
                .scheduled_task_run_get(&loupist.id, &run.run_id)?
                .ok_or("Briefing receipt disappeared")?;
            if current.status != crate::scheduled_tasks::types::RunStatus::Running {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(current);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    let receipt = match result {
        Ok(receipt) => receipt?,
        Err(_) => {
            let _ = state
                .cancel_run(&run.run_id, crate::runs::coordinator::RunKind::Chat)
                .await;
            state.shutdown().await?;
            state
                .store
                .scheduled_task_run_get(&loupist.id, &run.run_id)?
                .ok_or("Missing timed-out run")?
        }
    };
    let chat = state
        .store
        .get_chat(
            receipt
                .thread_id
                .as_deref()
                .ok_or("Briefing Chat missing")?,
        )?
        .ok_or("Briefing Chat missing")?;
    let message = chat
        .messages
        .iter()
        .rev()
        .find(|message| message.role == crate::domain::ChatRole::Assistant);
    let mut record = if let Some(message) = message {
        let mut record = trace::persisted(message, &captured.lock().unwrap());
        trace::enrich_commits(&state, &chat, message, &mut record)?;
        record
    } else {
        json!({"status":"failed","error":receipt.error,"answer":"","observed_model":null,"usage":{"input":null,"output":null,"cache_read":null,"cache_write":null}})
    };
    record["schema"] = json!("loupist-briefing-trace/1");
    record["model"] = json!({"provider":model.provider,"model":model.model,"thinking":"medium"});
    record["started_at"] = json!(started);
    record["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
    record["task"] = json!(task);
    record["inbox"] = json!(crate::briefings::commands::InboxItem::from(receipt));
    record["prompt"] = json!(chat
        .messages
        .iter()
        .find(|message| message.role == crate::domain::ChatRole::User)
        .map(|message| message.content.clone()));
    record["fixture"] = json!({"watchlist":["HANU","MORR","BLUE"],"source_count":3,"principle":"single customer revenue <= 25%","external_connections":[]});
    let mut bytes = Vec::new();
    append_atomic(&out, &mut bytes, &record)?;
    eprintln!(
        "briefing status={} model={} usage={}",
        record["status"], record["observed_model"], record["usage"]
    );
    state.shutdown().await?;
    if record["status"] != "completed" {
        return Err("Briefing did not complete; preserved trace".into());
    }
    Ok(())
}
