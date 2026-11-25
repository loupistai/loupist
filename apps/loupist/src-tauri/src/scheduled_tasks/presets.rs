use chrono::{NaiveDate, TimeZone};
use serde::Deserialize;

use super::types::*;
use crate::{
    app::{AppState, CommandError},
    commands::map_store,
};

#[derive(Clone, Debug, Default, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub(crate) struct PresetParams {
    pub language: Option<String>,
    pub results_date: Option<String>,
}

#[derive(Deserialize)]
struct Template {
    title: String,
    terms: String,
    prompt: String,
    report: ReportPolicy,
    #[serde(default)]
    days: Vec<u8>,
    #[serde(default)]
    hour: u8,
    #[serde(default)]
    minute: u8,
    #[serde(default)]
    every_minutes: Option<u32>,
    #[serde(default)]
    trigger: Option<String>,
}

pub(crate) struct FilledPreset {
    pub title: String,
    pub prompt: String,
    pub schedule: Schedule,
    pub watch: TaskWatch,
    /// A bundled check. The task is a proposal until the user turns it on.
    pub trigger_template: Option<String>,
}

pub(super) struct LiveScope {
    pub prompt: String,
    pub followed_ids: Option<Vec<String>>,
}

/// Resolve an occurrence against what the user still follows. The saved
/// definition remains intact. An empty explicit scope must never become an
/// implicit request to watch the whole list.
pub(super) fn live_scope(state: &AppState, task: &Task) -> Result<Option<LiveScope>, CommandError> {
    let followed = state
        .store
        .assets_list(&task.loupist_id, true)
        .map_err(map_store)?;
    scope_from_followed(task, &followed)
}

fn scope_from_followed(
    task: &Task,
    followed: &[crate::commands::assets::wire::AssetRow],
) -> Result<Option<LiveScope>, CommandError> {
    if let Some(ids) = price_move_followed_ids(&task.watch, followed) {
        return Ok((!ids.is_empty()).then(|| LiveScope {
            prompt: task.prompt.clone(),
            followed_ids: Some(ids),
        }));
    }
    let subjects = task.watch.subjects.as_deref().unwrap_or_default();
    if subjects.is_empty() {
        return Ok((!followed.is_empty()).then(|| LiveScope {
            prompt: task.prompt.clone(),
            followed_ids: None,
        }));
    }
    let mut ids = Vec::new();
    let names: Vec<_> = subjects
        .iter()
        .filter_map(|subject| match subject {
            TaskSubject::Asset { asset } => {
                let row = followed.iter().find(|row| &row.asset_id == asset)?;
                ids.push(row.asset_id.clone());
                Some(row.name.as_str())
            }
            TaskSubject::Name { name } => Some(name.as_str()),
        })
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    let prompt = if task.watch.preset == Some(WatchPreset::FilingsAndResults)
        && names.len() < subjects.len()
    {
        let template = template_for(WatchPreset::FilingsAndResults)?;
        refill_filings(&task.prompt, &template.prompt, &names)
            .unwrap_or_else(|| task.prompt.clone())
    } else {
        task.prompt.clone()
    };
    Ok(Some(LiveScope {
        prompt,
        followed_ids: Some(ids),
    }))
}

/// An explicit price watch stays scoped even when none of its subjects is followed.
pub(crate) fn price_move_followed_ids(
    watch: &TaskWatch,
    followed: &[crate::commands::assets::wire::AssetRow],
) -> Option<Vec<String>> {
    let subjects = watch.subjects.as_deref()?;
    if watch.preset != Some(WatchPreset::PriceMove) || subjects.is_empty() {
        return None;
    }
    Some(
        followed
            .iter()
            .filter(|row| {
                subjects.iter().any(|subject| match subject {
                    TaskSubject::Asset { asset } => *asset == row.asset_id,
                    TaskSubject::Name { name } => name.to_lowercase() == row.name.to_lowercase(),
                })
            })
            .map(|row| row.asset_id.clone())
            .collect(),
    )
}

fn refill_filings(saved: &str, template: &str, live_names: &[&str]) -> Option<String> {
    // Match the exact bundled wording, leaving a user-edited prompt alone.
    // The language and all wording outside the filled asset slot stay as saved.
    let (prefix, rest) = template.split_once("{assets}")?;
    let (middle, suffix) = rest.split_once("{language}")?;
    let (_, language) = saved
        .strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .rsplit_once(middle)?;
    valid_language(language).then(|| {
        format!(
            "{prefix}{}{middle}{language}{suffix}",
            live_names.join(", ")
        )
    })
}

fn valid_language(language: &str) -> bool {
    !language.is_empty()
        && language.chars().count() <= 32
        && language
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '-' | ' '))
}

pub(crate) fn fill(
    state: &AppState,
    loupist_id: &str,
    preset: WatchPreset,
    subjects: Option<Vec<TaskSubject>>,
    params: PresetParams,
    run_at_ms: Option<i64>,
) -> Result<FilledPreset, CommandError> {
    let mut language = crate::commands::loupist_configuration(state, loupist_id)?
        .appearance
        .language;
    crate::agent_harness::interface::resolve_language(&mut language);
    let followed = state
        .store
        .assets_list(loupist_id, true)
        .map_err(map_store)?;
    let mut subjects = subjects.unwrap_or_default();
    let initial_watch = TaskWatch {
        subjects: Some(subjects.clone()),
        ..Default::default()
    };
    initial_watch.validate().map_err(map_store)?;
    let requested_subjects = !subjects.is_empty();
    subjects.retain(|subject| match subject {
        TaskSubject::Asset { asset } => followed.iter().any(|row| &row.asset_id == asset),
        TaskSubject::Name { .. } => true,
    });
    if requested_subjects && subjects.is_empty() {
        return Err(CommandError::invalid("Nothing left to watch"));
    }
    let names = subjects
        .iter()
        .filter_map(|subject| match subject {
            TaskSubject::Asset { asset } => followed
                .iter()
                .find(|row| &row.asset_id == asset)
                .map(|row| row.name.as_str()),
            TaskSubject::Name { name } => Some(name.as_str()),
        })
        .collect::<Vec<_>>();
    fill_template(
        preset,
        subjects.clone(),
        &names,
        params,
        &language,
        local_timezone(),
        run_at_ms,
    )
}

fn local_timezone() -> chrono_tz::Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|zone| zone.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

pub(crate) fn results_follow_up_at(date: &str) -> Result<i64, CommandError> {
    follow_up_at(
        date,
        &template_for(WatchPreset::ResultsFollowUp)?,
        local_timezone(),
    )
}

fn follow_up_at(
    date: &str,
    template: &Template,
    timezone: chrono_tz::Tz,
) -> Result<i64, CommandError> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.succ_opt())
        .and_then(|date| date.and_hms_opt(template.hour.into(), template.minute.into(), 0))
        .and_then(|at| timezone.from_local_datetime(&at).earliest())
        .map(|at| at.timestamp_millis())
        .ok_or_else(|| CommandError::invalid("A results follow-up needs a valid results date"))
}

fn fill_template(
    preset: WatchPreset,
    subjects: Vec<TaskSubject>,
    names: &[&str],
    params: PresetParams,
    default_language: &str,
    timezone: chrono_tz::Tz,
    run_at_ms: Option<i64>,
) -> Result<FilledPreset, CommandError> {
    let template = template_for(preset)?;
    let language = params.language.as_deref().unwrap_or(default_language);
    if !valid_language(language) {
        return Err(CommandError::invalid("Invalid watch language"));
    }
    let schedule = if preset == WatchPreset::ResultsFollowUp {
        if names.len() != 1 {
            return Err(CommandError::invalid(
                "A results follow-up needs exactly one subject",
            ));
        }
        Schedule::Once {
            at_ms: if let Some(at) = run_at_ms {
                at
            } else {
                follow_up_at(
                    params.results_date.as_deref().unwrap_or(""),
                    &template,
                    timezone,
                )?
            },
        }
    } else {
        if params.results_date.is_some() {
            return Err(CommandError::invalid(
                "Only a results follow-up takes a results date",
            ));
        }
        if let Some(every_minutes) = template.every_minutes {
            Schedule::Interval { every_minutes }
        } else {
            Schedule::Weekly {
                days: template.days,
                hour: template.hour,
                minute: template.minute,
                timezone: timezone.to_string(),
            }
        }
    };
    let assets = if names.is_empty() {
        "every asset the user follows".to_owned()
    } else {
        names.join(", ")
    };
    let slot = |part: &str| match part {
        "asset" => names.first().copied().unwrap_or(""),
        "assets" => &assets,
        "results_date" => params.results_date.as_deref().unwrap_or(""),
        "language" => language,
        _ => "",
    };
    let render = |text: &str| {
        let mut chunks = text.split('{');
        let mut result = chunks.next().unwrap_or_default().to_owned();
        for chunk in chunks {
            if let Some((name, suffix)) = chunk.split_once('}') {
                result.push_str(slot(name));
                result.push_str(suffix);
            }
        }
        result
    };
    Ok(FilledPreset {
        title: render(&template.title),
        prompt: render(&template.prompt),
        schedule,
        watch: TaskWatch {
            delivery: None,
            report: Some(template.report),
            subjects: Some(subjects),
            preset: Some(preset),
            terms: Some(render(&template.terms).chars().take(160).collect()),
        },
        trigger_template: template.trigger,
    })
}

fn template_for(preset: WatchPreset) -> Result<Template, CommandError> {
    let bundled = match preset {
        WatchPreset::FilingsAndResults => {
            include_str!("../../../agent/watch-presets/filings-and-results.json")
        }
        WatchPreset::ThesisLines => include_str!("../../../agent/watch-presets/thesis-lines.json"),
        WatchPreset::ResultsFollowUp => {
            include_str!("../../../agent/watch-presets/results-follow-up.json")
        }
        WatchPreset::PriceMove => include_str!("../../../agent/watch-presets/price-move.json"),
        WatchPreset::WeekInReview => {
            include_str!("../../../agent/watch-presets/week-in-review.json")
        }
    };
    serde_json::from_str(bundled)
        .map_err(|_| CommandError::internal("Invalid bundled watch preset"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_price_move_preset_keeps_the_subject_and_resolves_only_that_follow() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let id = "named-price";
        crate::test_fixtures::create_folder_loupist(
            &state.store,
            id,
            &root.path().join("workspace"),
        );
        let named = state
            .store
            .seed_followed_listing(id, "AAA", "NASDAQ", Some("Northwind"))
            .unwrap();
        state
            .store
            .seed_followed_listing(id, "BBB", "NASDAQ", Some("Other"))
            .unwrap();
        let watch = TaskWatch {
            preset: Some(WatchPreset::PriceMove),
            subjects: Some(vec![TaskSubject::Name {
                name: "northwind".into(),
            }]),
            ..Default::default()
        };
        let check = crate::commands::scheduled_tasks::preset_trigger(
            &state,
            id,
            "price-band",
            &watch,
            "Price move",
        )
        .unwrap();
        assert_eq!(check.args["asset_names"], serde_json::json!(["northwind"]));
        let followed = state.store.assets_list(id, true).unwrap();
        assert_eq!(
            price_move_followed_ids(&watch, &followed),
            Some(vec![named.asset_id.clone()])
        );
        state
            .store
            .set_asset_followed(id, &named.asset_id, None)
            .unwrap();
        let followed = state.store.assets_list(id, true).unwrap();
        assert_eq!(price_move_followed_ids(&watch, &followed), Some(vec![]));
    }

    #[test]
    fn a_mixed_watch_drops_an_unfollowed_asset_and_keeps_its_language() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let id = "loupist-a";
        let workspace = root.path().join("workspace");
        crate::test_fixtures::create_folder_loupist(&state.store, id, &workspace);
        let gone = state
            .store
            .seed_followed_listing(id, "AAA", "NASDAQ", Some("Gone"))
            .unwrap();
        let live = state
            .store
            .seed_followed_listing(id, "BBB", "NASDAQ", Some("Live"))
            .unwrap();
        state
            .store
            .seed_followed_listing(id, "CCC", "NASDAQ", Some("Unrelated"))
            .unwrap();
        let subjects = vec![
            TaskSubject::Asset {
                asset: gone.asset_id.clone(),
            },
            TaskSubject::Asset {
                asset: live.asset_id.clone(),
            },
            TaskSubject::Name {
                name: "Explicit name".into(),
            },
        ];
        let preset = fill_template(
            WatchPreset::FilingsAndResults,
            subjects,
            &[&gone.name, &live.name, "Explicit name"],
            PresetParams {
                language: Some("Korean".into()),
                ..Default::default()
            },
            "English",
            chrono_tz::UTC,
            None,
        )
        .unwrap();
        let task = state
            .store
            .scheduled_task_create_definition(
                id,
                "watch",
                TaskDraft {
                    title: preset.title,
                    prompt: preset.prompt,
                    schedule: preset.schedule,
                    target: TaskTarget::NewChat,
                    model_profile_id: String::new(),
                    thinking_level: String::new(),
                    run_options: Default::default(),
                    use_memory: true,
                    update_memory: false,
                    status: TaskStatus::Active,
                },
                TaskKind::Custom,
                TaskDefinition {
                    watch: preset.watch,
                    ..Default::default()
                },
                100,
            )
            .unwrap();
        assert!(task.prompt.contains("Gone, Live, Explicit name"));
        state
            .store
            .set_asset_followed(id, &gone.asset_id, None)
            .unwrap();
        let scope = live_scope(&state, &task).unwrap().unwrap();
        assert!(scope.prompt.contains("among Live, Explicit name"));
        assert!(!scope.prompt.contains("Gone"));
        assert!(scope.prompt.ends_with("Answer in Korean."));
        assert_eq!(scope.followed_ids, Some(vec![live.asset_id]));
        let (claim, ()) = state
            .store
            .scheduled_task_claim_scoped(
                id,
                &task.id,
                101,
                false,
                super::super::store::ClaimScope::Definition(task.revision),
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claim.task.prompt, task.prompt);
        assert_eq!(
            state
                .store
                .scheduled_task_get(id, &task.id)
                .unwrap()
                .unwrap()
                .prompt,
            task.prompt
        );
        let edited = format!("{}\nRead the full appendix.", task.prompt);
        let template = template_for(WatchPreset::FilingsAndResults).unwrap();
        assert!(refill_filings(&edited, &template.prompt, &["Live"]).is_none());
    }

    #[test]
    fn presets_fill_their_scope_and_pin_the_follow_up_to_the_local_morning() {
        for preset in [
            WatchPreset::FilingsAndResults,
            WatchPreset::ThesisLines,
            WatchPreset::ResultsFollowUp,
            WatchPreset::WeekInReview,
        ] {
            let follow_up = preset == WatchPreset::ResultsFollowUp;
            let params = PresetParams {
                language: Some("Korean".into()),
                results_date: follow_up.then(|| "2027-01-15".into()),
            };
            let filled = fill_template(
                preset,
                vec![TaskSubject::Name {
                    name: "Northwind".into(),
                }],
                &["Northwind"],
                params,
                "en",
                chrono_tz::Asia::Seoul,
                None,
            )
            .unwrap();
            assert_eq!(filled.watch.preset, Some(preset));
            assert!(filled.watch.terms.unwrap().contains("Northwind"));
            assert!(filled.prompt.ends_with("Answer in Korean."));
            if follow_up {
                assert!(filled.prompt.contains("on\n2027-01-15"));
                assert_eq!(
                    filled.schedule,
                    Schedule::Once {
                        at_ms: chrono_tz::Asia::Seoul
                            .with_ymd_and_hms(2027, 1, 16, 8, 0, 0)
                            .unwrap()
                            .timestamp_millis(),
                    }
                );
            }
        }
    }
}
