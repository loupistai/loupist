#[cfg(feature = "eval-harness")]
pub mod eval_harness;
#[cfg(all(feature = "eval-harness", not(debug_assertions)))]
compile_error!("eval-harness is forbidden in release builds");
mod agent_harness;
mod app;
#[cfg(test)]
mod bridge_contract;
mod briefings;
mod broker;
mod calculations;
mod chat;
mod commands;
mod custom_connections;
mod datasets;
mod domain;
mod events;
pub mod finance_credentials;
mod finance_data;
mod fs;
mod knowledge;
mod market_calendar;
mod marketplace;
mod mcp;
mod pi;
mod power;
mod privacy;
mod process_lease;
#[cfg(test)]
mod product_contract;
mod providers;
mod python_run;
mod runs;
mod scheduled_tasks;
mod scoped_config;
mod sharing;
mod skills;
mod store;
#[cfg(test)]
mod test_fixtures;
mod updater;
mod util;
mod web;
#[cfg(feature = "webdriver")]
mod webdriver_attach;

#[cfg(all(feature = "webdriver", not(debug_assertions)))]
compile_error!("WebDriver is forbidden in release builds");

#[cfg(any(not(feature = "e2e"), test))]
pub(crate) const PRODUCTION_APP_IDENTIFIER: &str = "com.monarchjuno.loupist";
#[cfg(feature = "e2e")]
pub(crate) const E2E_APP_IDENTIFIER: &str = "com.monarchjuno.loupist.e2e";

fn install_app_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::Emitter;

    let new_chat = MenuItem::with_id(app, "new-chat", "New Chat", true, Some("CmdOrCtrl+N"))?;
    let new_loupist = MenuItem::with_id(app, "new-loupist", "Open folder…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let updates = MenuItem::with_id(
        app,
        "check-updates",
        "Check for Updates…",
        true,
        None::<&str>,
    )?;
    let sidebar = MenuItem::with_id(
        app,
        "toggle-sidebar",
        "Toggle Sidebar",
        true,
        Some("CmdOrCtrl+B"),
    )?;
    let chat = MenuItem::with_id(app, "chat", "Chat", true, Some("CmdOrCtrl+1"))?;
    let map = MenuItem::with_id(app, "map", "Map", true, Some("CmdOrCtrl+2"))?;
    let lens = MenuItem::with_id(app, "lens", "Lens", true, Some("CmdOrCtrl+3"))?;
    let schedule = MenuItem::with_id(app, "schedule", "Schedule", true, Some("CmdOrCtrl+4"))?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let file = Submenu::with_items(app, "File", true, &[&new_chat, &new_loupist])?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &sidebar,
            &PredefinedMenuItem::separator(app)?,
            &chat,
            &map,
            &lens,
            &schedule,
        ],
    )?;
    let app_menu = Submenu::with_items(
        app,
        "Loupist",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("Loupist"), None)?,
            &settings,
            &updates,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &file, &edit, &view])?)?;
    app.on_menu_event(|app, event| {
        let _ = app.emit(events::MENU_ACTION, event.id().as_ref());
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    let builder = tauri::Builder::default();
    // `main` starts the Windows UI on an explicitly sized worker thread so
    // debug startup has enough stack. Opt Tauri's event loop into that
    // supported execution mode before constructing windows or plugins.
    #[cfg(windows)]
    let builder = builder.any_thread();
    // Isolated E2E profiles may run alongside the development window. Their
    // app-data and Loupist folder locks still prevent shared storage writers.
    #[cfg(all(any(target_os = "macos", windows), not(feature = "e2e")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        use tauri::Manager;

        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }));
    // `loupist://import?url=…` links (W19.8). Rust validates every URL before
    // the renderer hears of one; nothing is fetched until Preview.
    #[cfg(all(any(target_os = "macos", windows), not(feature = "e2e")))]
    let builder = builder.plugin(tauri_plugin_deep_link::init());
    #[cfg(all(any(target_os = "macos", windows), not(debug_assertions)))]
    let builder = if updater::configured(&context.config().plugins, env!("CARGO_PKG_VERSION")) {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    #[cfg(feature = "webdriver")]
    let builder = {
        #[cfg(feature = "e2e")]
        assert_eq!(
            context.config().identifier,
            E2E_APP_IDENTIFIER,
            "the e2e WebDriver requires the isolated Loupist E2E identifier"
        );
        #[cfg(not(feature = "e2e"))]
        assert_eq!(
            context.config().identifier,
            PRODUCTION_APP_IDENTIFIER,
            "debug WebDriver attaches only to the development app identifier"
        );
        webdriver_attach::wait_for_bind_port();
        builder.plugin(tauri_plugin_wdio_webdriver::init())
    };

    builder
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            briefings::desktop::close_requested(window, event);
            commands::attention::window_event(window, event);
        })
        .setup(|app| {
            use tauri::Manager;

            let state = match app::AppState::initialize(app.handle()) {
                Ok(state) => state,
                Err(error) if app::is_app_instance_conflict(&error) => {
                    // The official single-instance plugin normally exits before
                    // setup after notifying the primary process. Its macOS Unix
                    // socket can nevertheless be absent during a simultaneous
                    // launch or after an older build, while the independent
                    // app-data lock still proves that a primary process is live.
                    // Treat only that exact lock conflict as a normal relaunch;
                    // returning the error from a macOS setup hook would panic in
                    // the native event-loop callback and show a crash report.
                    app.cleanup_before_exit();
                    std::process::exit(0);
                }
                Err(error) => {
                    eprintln!("Loupist failed to start: {error}");
                    // No window exists yet: a newer or failed-upgrade catalog
                    // would otherwise quit without telling the user why.
                    if let Some((title, body)) = app::startup_storage_notice(&error) {
                        show_startup_notice(title, &body);
                    }
                    app.cleanup_before_exit();
                    std::process::exit(1);
                }
            };
            if let Err(error) = tauri::async_runtime::block_on(async {
                if state.fresh_install {
                    #[cfg(feature = "e2e")]
                    {
                        commands::seed_e2e_workspace(&state).await?;
                        commands::knowledge::seed_e2e_knowledge_fixture(&state)?;
                    }
                }
                state.scheduled.start(&state)?;
                let mut learning_events = state.graph.subscribe_learning();
                let learning_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match learning_events.recv().await {
                            Ok(event) => { let _ = tauri::Emitter::emit(&learning_app, events::LEARNING_REVIEW_FINISHED, event); }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                });
                let mut schema_events = state.graph.subscribe_schema();
                let schema_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match schema_events.recv().await {
                            Ok(event) => { let _ = tauri::Emitter::emit(&schema_app, events::WORLD_SCHEMA_CHANGED, event); }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                });
                state.graph.start().map_err(|error| app::CommandError::internal(error.to_string()))?;
                Ok::<(), app::CommandError>(())
            }) {
                eprintln!("Loupist failed to start: {error}");
                app.cleanup_before_exit();
                std::process::exit(1);
            }
            let update_coordinator = state.updates.clone();
            app.manage(state);
            pi::shell_path::prewarm();
            providers::status::install(app.handle());
            if let Err(error) = briefings::desktop::install(app.handle()) {
                eprintln!("Briefing tray could not be installed: {error}");
            }
            if let Err(error) = install_app_menu(app.handle()) {
                eprintln!("Loupist menu was not installed: {error}");
            }
            updater::start_auto_update_loop(app.handle().clone(), update_coordinator);
            #[cfg(all(any(target_os = "macos", windows), not(feature = "e2e")))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let links = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    sharing::commands::deep_links::deliver_links(
                        &links,
                        event.urls().into_iter().map(|url| url.to_string()),
                    );
                });
                // A link that launched the app is held until onboarding asks for it.
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    sharing::commands::deep_links::deliver_links(
                        app.handle(),
                        urls.into_iter().map(|url| url.to_string()),
                    );
                }
            }
            #[cfg(feature = "webdriver")]
            webdriver_attach::require_status_endpoint();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            briefings::commands::briefing_settings_get,
            briefings::commands::briefing_settings_set,
            briefings::commands::inbox_list,
            briefings::commands::inbox_mark_read,
            briefings::commands::inbox_unread_counts,
            briefings::desktop::briefing_resident_settings_get,
            briefings::desktop::briefing_resident_settings_set,
            commands::loupist::loupist_list,
            commands::loupist::loupist_select,
            commands::loupist::loupist_create,
            commands::loupist::loupist_import,
            commands::loupist::loupist_profile_update,
            commands::loupist::loupist_choose_folder,
            commands::dialog::pick_file,
            commands::export_file::export_save,
            commands::loupist::loupist_reconnect,
            commands::loupist::loupist_delete,
            commands::loupist::agent_skill_catalog,
            commands::skills::skills_list,
            commands::skills::skills_read,
            commands::skills::skills_create,
            commands::skills::skills_save,
            commands::skills::skills_delete,
            commands::skills::skills_import,
            commands::skills::skills_reveal,
            commands::approvals::chat_approval_decide,
            commands::questions::chat_question_answer,
            commands::attention::attention_list,
            commands::assets::asset_page_get,
            commands::assets::asset_delta,
            commands::assets::upcoming_events,
            commands::assets::asset_refresh,
            commands::earnings::earnings_snapshot_get,
            commands::earnings::earnings_events_list,
            commands::earnings::earnings_prepare,
            commands::watch::watch_defaults_apply,
            commands::watch::watch_settings_list,
            commands::watch::watch_setting_set,
            commands::watch::watch_pause_set,
            commands::watch::watch_check_now,
            commands::watch::watch_events_list,
            commands::watch::watch_events_seen,
            commands::assets::asset_class_set,
            commands::assets::asset_verdict_undo,
            commands::assets::asset_page_seen,
            commands::attention::attention_notify,
            commands::scheduled_tasks::scheduled_task_run_now,
            commands::scheduled_tasks::scheduled_tasks_activity,
            commands::scheduled_tasks::scheduled_task_watch,
            commands::scheduled_tasks::scheduled_task_get,
            commands::scheduled_tasks::scheduled_task_confirm,
            commands::scheduled_tasks::scheduled_task_discard,
            commands::scheduled_tasks::scheduled_tasks_list,
            commands::scheduled_tasks::scheduled_task_set_status,
            commands::scheduled_tasks::scheduled_task_delete,
            commands::scheduled_tasks::scheduled_task_runs,
            commands::records::chat_create,
            commands::records::chat_rename,
            commands::records::chat_feedback_set,
            commands::records::chat_memory_set_aside,
            commands::records::chat_branch,
            commands::records::chat_version_switch,
            commands::records::chat_delete,
            commands::records::chat_attachment_read,
            commands::records::chat_artifact_list,
            commands::records::loupist_artifact_list,
            commands::records::chat_artifact_read,
            commands::records::artifact_versions,
            commands::records::artifact_restore,
            commands::workspace::workspace_list,
            commands::workspace::workspace_read,
            commands::workspace::workspace_reveal,
            commands::workspace::chat_activity_read,
            commands::workspace_ui::chat_draft_files_read,
            commands::workspace_ui::chat_draft_files_write,
            commands::workspace_ui::chat_draft_read,
            commands::workspace_ui::chat_draft_write,
            commands::workspace_ui::workspace_ui_read,
            commands::workspace_ui::workspace_ui_write,
            commands::chat_runtime::chat_send,
            commands::chat_runtime::chat_steer,
            commands::chat_runtime::chat_retry_last,
            commands::handoff::research_handoff_start,
            commands::chat_runtime::chat_abort,
            commands::knowledge::knowledge_catalog,
            commands::knowledge::knowledge_growth_events,
            commands::knowledge::knowledge_world_query,
            commands::knowledge::knowledge_map,
            commands::knowledge::knowledge_source_read,
            commands::knowledge::source_selection::knowledge_source_selection_resolve,
            commands::knowledge::source_export::knowledge_source_export,
            commands::knowledge::method_learning::learning_review_list,
            commands::knowledge::method_learning::memory_learning_settings_get,
            commands::knowledge::method_learning::memory_learning_settings_set,
            commands::knowledge::journal::journal_list,
            commands::knowledge::journal::journal_create,
            commands::knowledge::journal::journal_update,
            commands::knowledge::journal::journal_archive,
            commands::knowledge::principles::principle_list,
            commands::knowledge::principles::principle_create,
            commands::knowledge::principles::principle_update,
            commands::knowledge::principles::principle_archive,
            commands::knowledge::lens_archive::knowledge_lens_archive,
            commands::knowledge::memory_usage::knowledge_memory_usage,
            commands::knowledge::memory_controls::knowledge_memory_controls,
            commands::knowledge::memory_controls::knowledge_lens_pause,
            commands::knowledge::memory_controls::knowledge_memory_reset_preview,
            commands::knowledge::memory_controls::knowledge_memory_reset,

            commands::knowledge::knowledge_lens_read,
            commands::knowledge::knowledge_lens_search,
            commands::knowledge::knowledge_lens_pages_for_node,
            commands::knowledge::knowledge_connection_dismiss,
            commands::knowledge::knowledge_connection_open,
            commands::knowledge::memory_section_forget,
            commands::knowledge::memory_op_undo,
            commands::knowledge::memory_learned,
            commands::knowledge::knowledge_research_search,
            commands::knowledge::working_notes::knowledge_working_notes,
            commands::knowledge::working_notes::knowledge_working_note_read,
            commands::knowledge::knowledge_journal_read,
            commands::knowledge::knowledge_journal_source_read,
            commands::knowledge::knowledge_source_retain,
            commands::knowledge::knowledge_graph_job,
            commands::knowledge::followed::knowledge_followed_status,
            commands::knowledge::knowledge_graph_recover,
            commands::knowledge::knowledge_graph_pause,
            commands::knowledge::knowledge_graph_settings_get,
            commands::knowledge::knowledge_graph_settings_set,
            commands::power::world_keep_awake_get,
            commands::power::world_keep_awake_set,
            commands::knowledge::world_schema::world_schema_get,
            commands::knowledge::world_schema::world_schema_undo,
            commands::knowledge::world_schema::world_schema_remove,
            commands::model_catalog_get,
            commands::user_context::watchlist_list,
            commands::user_context::watchlist_add,
            commands::user_context::watchlist_remove,
            commands::user_context::holdings_import_csv,
            commands::user_context::holdings_commit_csv,
            commands::user_context::holdings_import_kis,
            commands::user_context::holdings_summary,
            commands::user_context::holdings_import_crypto_file,
            commands::user_context::holdings_map_columns,
            commands::user_context::wallets_list,
            commands::user_context::wallet_add,
            commands::user_context::wallet_rename,
            commands::user_context::wallet_remove,
            commands::user_context::wallet_refresh,
            commands::app_settings::notice_settings_get,
            commands::app_settings::notice_acknowledge,
            commands::configuration::config_read,
            commands::configuration::config_update,
            commands::configuration::config_reveal,
            runs::log::run_logs_reveal,
            runs::log::run_logs_last_error,
            commands::model_visibility_update,
            commands::run_activity_list,
            providers::connection::provider_models,
            providers::connection::provider_configure,
            providers::connection::provider_custom_save,
            providers::connection::provider_custom_remove,
            providers::connection::provider_connect,
            providers::connection::provider_connect_cancel,
            providers::connection::provider_connect_open_browser,
            providers::connection::provider_connect_submit_code,
            providers::connection::provider_disconnect,
            providers::connection::provider_open_console,
            providers::connection::provider_open_usage,
            providers::connection::model_notice_ack,
            providers::local::provider_local_discover,
            providers::local::provider_custom_test,
            providers::local::provider_preset_save,
            marketplace::marketplace_snapshot,
            custom_connections::commands::custom_connection_add_url,
            custom_connections::commands::custom_connection_add_json,
            custom_connections::commands::custom_connection_add_command,
            custom_connections::commands::custom_connection_confirm,
            custom_connections::commands::custom_connection_list,
            custom_connections::commands::custom_connection_remove,
            custom_connections::commands::custom_connection_set_memory,
            custom_connections::commands::custom_connection_set_enabled,
            custom_connections::commands::custom_connection_authorize,
            custom_connections::commands::custom_connection_sign_in,
            custom_connections::commands::custom_connection_sign_in_cancel,
            custom_connections::commands::custom_tool_approve,
            marketplace::loupist_capability_list,
            marketplace::loupist_capability_enable,
            marketplace::connector_config::marketplace_connector_configure,
            marketplace::loupist_capability_disable,
            marketplace::credentials::marketplace_credential_save,
            marketplace::credentials::marketplace_credential_verify,
            marketplace::credentials::marketplace_credential_delete,
            commands::open_external_url,
            web::favicon::site_favicon,
            web::browser::browser_tab_open,
            web::browser::browser_tab_navigate,
            web::browser::browser_tab_history,
            web::browser::browser_tab_reload,
            web::browser::browser_tab_set_bounds,
            web::browser::browser_tab_close,
            web::browser::browser_tabs_reset,
            updater::update_status,
            updater::update_check,
            updater::update_install,
            sharing::commands::share::share_summary,
            sharing::commands::save::share_export,
            sharing::commands::share::share_copy_text,
            sharing::commands::save::share_reveal,
            sharing::commands::publish::share_publish,
            sharing::commands::github_account::github_account,
            sharing::commands::github_account::github_sign_in,
            sharing::commands::github_account::github_sign_in_open,
            sharing::commands::github_account::github_sign_in_cancel,
            sharing::commands::github_account::github_sign_out,
            sharing::commands::open::pack_open,
            sharing::commands::add::pack_add,
            sharing::commands::added::pack_list,
            sharing::commands::review::pack_suggestions,
            sharing::commands::review::pack_item_read,
            sharing::commands::review::pack_decide,
            sharing::commands::added::pack_remove,
            sharing::commands::added::pack_report,
            sharing::commands::added::pack_origin,
            sharing::commands::updates::pack_check_update,
            sharing::commands::updates::pack_choose_newer,
            sharing::commands::updates::pack_apply_update,
            sharing::commands::gallery::gallery_index,
            sharing::commands::gallery::gallery_image,
            sharing::commands::gallery::gallery_listing_status,
            sharing::commands::gallery::gallery_listing_request,
            sharing::commands::deep_links::deep_link_pending,
            sharing::commands::deep_links::deep_link_dismiss,
        ])
        .build(context)
        .expect("failed to build Loupist")
        .run(|handle, event| {
            #[cfg(target_os = "macos")]
            if matches!(event, tauri::RunEvent::Reopen { .. }) {
                briefings::desktop::open(handle, None);
            }
            if matches!(event, tauri::RunEvent::Exit) {
                use tauri::Manager;

                if let Some(state) = handle.try_state::<app::AppState>() {
                    // Tauri exits the process directly after this callback;
                    // owned sidecars must stop before that boundary.
                    let stopped = tauri::async_runtime::block_on(async {
                        tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            state.shutdown(),
                        )
                        .await
                    });
                    if !matches!(stopped, Ok(Ok(()))) {
                        eprintln!(
                            "Loupist sidecar shutdown did not complete; process leases were kept for recovery"
                        );
                    }
                }
            }
        });
}

/// A blocking native alert shown before the app exits during startup.
fn show_startup_notice(title: &str, body: &str) {
    // The E2E harness drives no native dialogs; the message is in the log.
    #[cfg(not(feature = "e2e"))]
    {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(title)
            .set_description(body)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    #[cfg(feature = "e2e")]
    let _ = (title, body);
}
