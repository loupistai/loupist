#[cfg(feature = "eval-harness")]
pub mod eval_harness;
#[cfg(all(feature = "eval-harness", not(debug_assertions)))]
compile_error!("eval-harness is forbidden in release builds");
mod agent_harness;
mod app;
mod app_menu;
#[cfg(test)]
mod bridge_contract;
mod briefings;
mod broker;
#[cfg(target_os = "macos")]
mod bundle_self_test;
mod calculations;
mod chat;
mod commands;
mod custom_connections;
mod datasets;
mod domain;
mod events;
mod extension_availability;
pub mod finance_credentials;
mod finance_data;
mod fs;
mod keychain_operation;
mod knowledge;
mod local_time;
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

#[cfg(any(target_os = "macos", not(feature = "e2e"), test))]
pub(crate) const PRODUCTION_APP_IDENTIFIER: &str = "com.monarchjuno.loupist";
#[cfg(feature = "e2e")]
pub(crate) const E2E_APP_IDENTIFIER: &str = "com.monarchjuno.loupist.e2e";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Run before Tauri construction, single-instance registration or app-data
    // recovery: preflight must never open the production profile or a window.
    #[cfg(target_os = "macos")]
    if let Some(result) = bundle_self_test::requested(std::env::args_os()) {
        if let Err(error) = &result {
            eprintln!("Release helper preflight failed: {error}");
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let context = tauri::generate_context!();
    // Configured windows are created before Tauri calls setup. Defer presentation
    // only for the installed Mac app so a login launch never flashes the window.
    #[cfg(target_os = "macos")]
    let (context, deferred_window) = {
        let mut context = context;
        let main = context
            .config_mut()
            .app
            .windows
            .iter_mut()
            .find(|w| w.label == "main");
        let deferred = main.is_some_and(|window| {
            let defer = window.visible && briefings::login_item::macos::available();
            if defer {
                window.visible = false;
            }
            defer
        });
        (context, deferred)
    };
    #[cfg(target_os = "macos")]
    app_menu::macos::disable_automatic_menu_items();
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

    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        // Registering this hook replaces Tauri's default macOS reload.
        app_menu::renderer_reset(webview);
        briefings::desktop::renderer_reset(webview);
        let _ = webview.reload();
    });
    builder
        .plugin(tauri_plugin_notification::init())
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                app_menu::renderer_reset(webview);
                briefings::desktop::renderer_reset(webview);
            }
        })
        .on_window_event(|window, event| {
            briefings::desktop::window_event(window, event);
            commands::attention::window_event(window, event);
            app_menu::window_event(window, event);
        })
        .setup(move |app| {
            use tauri::Manager;

            #[cfg(target_os = "macos")]
            let login_launch = deferred_window && briefings::login_item::macos::login_launch();
            #[cfg(target_os = "macos")]
            if deferred_window && !login_launch {
                briefings::desktop::open(app.handle(), None);
            }
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
            // The embedding model takes seconds to load; searches before then
            // rank by words alone, so start it now on its own thread.
            state.graph.prewarm_semantic_model();
            if let Err(error) = tauri::async_runtime::block_on(async {
                #[cfg(feature = "e2e")]
                commands::handoff_fixture::prepare(&state)?;
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
                            Ok(event) => {
                                let _ = tauri::Emitter::emit(
                                    &learning_app,
                                    events::LEARNING_REVIEW_FINISHED,
                                    event,
                                );
                            }
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
                            Ok(event) => {
                                let _ = tauri::Emitter::emit(
                                    &schema_app,
                                    events::WORLD_SCHEMA_CHANGED,
                                    event,
                                );
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                });
                state
                    .graph
                    .start()
                    .map_err(|error| app::CommandError::internal(error.to_string()))?;
                Ok::<(), app::CommandError>(())
            }) {
                eprintln!("Loupist failed to start: {error}");
                app.cleanup_before_exit();
                std::process::exit(1);
            }
            // Sessions earlier runs made and no longer need go off the launch path.
            let release_state = state.clone();
            tauri::async_runtime::spawn_blocking(move || {
                commands::records::release_host_sessions(&release_state);
            });
            // This build's World model list applies to every Recommended Loupist.
            let world_state = state.clone();
            tauri::async_runtime::spawn_blocking(move || {
                commands::knowledge::follow_recommended_world_models_quietly(&world_state);
            });
            // Start any due launch check before the renderer can read update status.
            updater::start_auto_update_loop(app.handle().clone(), state.updates.clone());
            app.manage(state);
            let catalog_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = catalog_app.state::<app::AppState>();
                providers::connection::refresh_upgraded_models(&state, || {
                    let _ = tauri::Emitter::emit(&catalog_app, events::MODEL_CATALOG_CHANGED, ());
                })
                .await;
            });
            pi::shell_path::prewarm();
            // The market-data runtime takes tens of seconds to start on a busy
            // Mac. Start it off the launch path, after the first window has
            // had a moment to paint; nothing starts when no Loupist enables it.
            let market_data_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let state = market_data_app.state::<app::AppState>();
                commands::prewarm_market_data(&state).await;
            });
            providers::status::install(app.handle());
            let tray = briefings::desktop::install(app.handle());
            #[cfg(target_os = "macos")]
            if login_launch && tray.is_err() {
                briefings::desktop::open(app.handle(), None);
            }
            if let Err(error) = tray {
                eprintln!("Briefing tray could not be installed: {error}");
            }
            if let Err(error) = app_menu::install(app.handle()) {
                eprintln!("Loupist menu was not installed: {error}");
            }
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
            commands::legacy_holdings::start(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_menu::app_menu_update,
            #[cfg(all(target_os = "macos", feature = "e2e"))]
            app_menu::macos::e2e_native_menu_snapshot,
            commands::cards::bot_offer_accept,
            commands::cards::bot_offer_dismiss,
            commands::cards::bot_ask_answer,
            briefings::commands::briefing_settings_get,
            briefings::commands::briefing_settings_set,
            briefings::commands::inbox_list,
            briefings::commands::inbox_mark_read,
            briefings::commands::inbox_unread_counts,
            briefings::desktop::briefing_resident_settings_get,
            briefings::desktop::briefing_resident_settings_set,
            briefings::login_item::login_item_set,
            briefings::login_item::login_item_hint_dismiss,
            briefings::login_item::login_item_open_settings,
            commands::loupist::loupist_list,
            commands::loupist::loupist_select,
            commands::loupist::loupist_create,
            commands::loupist::loupist_import,
            commands::loupist::loupist_profile_update,
            commands::loupist::loupist_choose_folder,
            commands::dialog::pick_file,
            commands::clipboard::clipboard_read_image,
            commands::export_file::export_save,
            commands::scripts::scripts_list,
            commands::scripts::script_get,
            commands::scripts::script_save,
            commands::scripts::script_delete,
            commands::scripts::script_test,
            commands::scripts::python_step_replay,
            commands::legacy_holdings::legacy_holdings_status,
            commands::legacy_holdings::legacy_holdings_retry,
            commands::legacy_holdings::legacy_holdings_reveal,
            commands::legacy_holdings::legacy_holdings_save_copy,
            commands::legacy_holdings::legacy_holdings_dismiss,
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
            commands::assets::assets_list,
            commands::assets::asset_follow_name,
            commands::assets::asset_unfollow,
            commands::assets::asset_lookup,
            commands::assets::asset_price_check,
            commands::assets::asset_page_get,
            commands::assets::asset_entity_set,
            commands::assets::asset_follow,
            commands::assets::watchlist_quotes,
            commands::assets::asset_price_series,
            commands::assets::asset_verdict_undo,
            commands::assets::asset_page_seen,
            commands::attention::attention_notify,
            commands::scheduled_tasks::scheduled_task_run_now,
            commands::scheduled_tasks::scheduled_tasks_activity,
            commands::scheduled_tasks::scheduled_task_watch,
            commands::scheduled_tasks::scheduled_task_get,
            commands::scheduled_tasks::scheduled_task_from_preset,
            commands::scheduled_tasks::scheduled_trigger_get,
            commands::scheduled_tasks::scheduled_trigger_approve,
            commands::scheduled_tasks::scheduled_trigger_revert,
            commands::scheduled_tasks::scheduled_trigger_test,
            commands::scheduled_tasks::scheduled_trigger_evaluations,
            briefings::desktop::notification_templates_set,
            briefings::desktop::notification_bot_focus_set,
            briefings::desktop::notification_permission_settings_open,
            commands::scheduled_tasks::scheduled_task_set_delivery,
            commands::scheduled_tasks::scheduled_task_confirm,
            commands::scheduled_tasks::scheduled_task_discard,
            commands::scheduled_tasks::scheduled_proposal_set_declined,
            commands::scheduled_tasks::scheduled_proposal_visit_end,
            commands::scheduled_tasks::scheduled_tasks_list,
            commands::scheduled_tasks::scheduled_task_set_status,
            commands::scheduled_tasks::learning_program_update,
            commands::scheduled_tasks::learning_program_undo_all,
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
            commands::workspace::workspace_workbook_read,
            commands::workspace::workspace_document_handler,
            commands::workspace::workspace_document_open,
            commands::workspace::workspace_reveal,
            commands::workspace::chat_activity_read,
            commands::workspace_ui::chat_draft_files_read,
            commands::workspace_ui::chat_draft_files_write,
            commands::workspace_ui::chat_draft_read,
            commands::workspace_ui::chat_draft_write,
            commands::workspace_ui::workspace_ui_read,
            commands::workspace_ui::composer_model::chat_composer_model_read,
            commands::workspace_ui::composer_model::chat_composer_model_write,
            commands::workspace_ui::workspace_ui_write,
            commands::chat_runtime::chat_send,
            commands::chat_runtime::chat_watch,
            commands::chat_runtime::chat_unwatch,
            commands::chat_runtime::chat_steer,
            commands::chat_runtime::chat_retry_last,
            commands::handoff::research_handoff_start,
            commands::chat_runtime::chat_abort,
            commands::chat_runtime::saves::chat_saves,
            commands::knowledge::knowledge_catalog,
            commands::knowledge::knowledge_growth_events,
            commands::knowledge::knowledge_learning_state,
            commands::knowledge::knowledge_world_query,
            commands::knowledge::world_changes::knowledge_world_changes,
            commands::knowledge::world_changes::knowledge_world_claim_history,
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
            commands::knowledge::memory_section_forget,
            commands::knowledge::memory_message_undo,
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
            commands::knowledge::knowledge_graph_retry_source,
            commands::knowledge::knowledge_graph_retry_sources,
            commands::knowledge::knowledge_graph_settings_get,
            commands::knowledge::knowledge_graph_settings_set,
            commands::power::world_keep_awake_get,
            commands::power::world_keep_awake_set,
            commands::knowledge::world_schema::world_schema_get,
            commands::knowledge::world_schema::world_schema_undo,
            commands::knowledge::world_schema::world_schema_remove,
            commands::model_catalog_get,
            commands::fx_display::fx_display,
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
            commands::provider_quota::provider_quota_read,
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
            commands::usage::usage_summary,
            commands::bot_memory::bot_memory_list,
            commands::bot_memory::bot_memory_forget,
            commands::bot_memory::bot_memory_update,
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
            marketplace::credentials::marketplace_credential_unlock,
            marketplace::key_permission::marketplace_credentials_allow_access,
            marketplace::key_permission::marketplace_credentials_cancel_access,
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
                    if tauri::async_runtime::block_on(state.shutdown()).is_err() {
                        eprintln!("Loupist shutdown did not finish draining work");
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
