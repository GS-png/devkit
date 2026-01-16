use crate::config::AppState;
use crate::ui::AudioController;
use crate::app::{setup::setup_application, commands::*};
use crate::log_important;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tauri::Builder;

/// 构建Tauri应用
pub fn build_tauri_app() -> Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())

        .manage(AppState::default())
        .manage(AudioController {
            should_stop: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            // 基础应用命令
            get_app_info,
            get_always_on_top,
            set_always_on_top,
            sync_window_state,
            reload_config,

            // 音频命令
            get_audio_notification_enabled,
            set_audio_notification_enabled,
            get_audio_url,
            set_audio_url,
            play_notification_sound,
            test_audio_sound,
            stop_audio_sound,
            get_available_audio_assets,
            refresh_audio_assets,

            // 主题和窗口命令
            get_theme,
            set_theme,
            get_window_config,
            set_window_config,
            get_reply_config,
            set_reply_config,
            get_window_settings,
            set_window_settings,
            get_window_settings_for_mode,
            get_window_constraints_cmd,
            get_current_window_size,
            apply_window_constraints,
            update_window_size,

            // 字体命令
            get_font_config,
            set_font_family,
            set_font_size,
            set_custom_font_family,
            get_font_family_options,
            get_font_size_options,
            reset_font_config,

            // MCP 命令
            get_mcp_tools_config,
            set_mcp_tool_enabled,
            get_mcp_tools_status,
            reset_mcp_tools_config,
            get_interaction_wait_ms,
            set_interaction_wait_ms,
            list_bistro_journal_entries,
            get_bistro_journal_entry,
            delete_bistro_journal_entry,
            delete_bistro_journal_by_time_range,
            export_bistro_journal_entry_zip,
            export_bistro_journal_by_time_range_zip,
            send_mcp_response,
            get_cli_args,
            read_mcp_request,
            stash_ingredient_bytes_cmd,
            stash_ingredient_base64_cmd,
            discard_spice_cmd,
            read_clipboard_ingredients_cached,
            build_mcp_continue_response,
            create_test_popup,
            
            // acemcp命令（迁移至 tools::acemcp::commands）
            crate::mcp::tools::acemcp::commands::get_acemcp_config,
            crate::mcp::tools::acemcp::commands::save_acemcp_config,
            crate::mcp::tools::acemcp::commands::test_acemcp_connection,
            crate::mcp::tools::acemcp::commands::read_acemcp_logs,
            crate::mcp::tools::acemcp::commands::clear_acemcp_cache,
            crate::mcp::tools::acemcp::commands::debug_acemcp_search,
            crate::mcp::tools::acemcp::commands::execute_acemcp_tool,
            crate::mcp::tools::acemcp::commands::get_acemcp_index_status,
            crate::mcp::tools::acemcp::commands::get_all_acemcp_index_status,
            crate::mcp::tools::acemcp::commands::get_acemcp_project_files_status,
            crate::mcp::tools::acemcp::commands::trigger_acemcp_index_update,
            crate::mcp::tools::acemcp::commands::get_auto_index_enabled,
            crate::mcp::tools::acemcp::commands::set_auto_index_enabled,
            crate::mcp::tools::acemcp::commands::get_watching_projects,
            crate::mcp::tools::acemcp::commands::is_project_watching,
            crate::mcp::tools::acemcp::commands::start_project_watching,
            crate::mcp::tools::acemcp::commands::stop_project_watching,
            crate::mcp::tools::acemcp::commands::stop_all_watching,
            crate::mcp::tools::acemcp::commands::remove_acemcp_project_index,
            crate::mcp::tools::acemcp::commands::check_directory_exists,
            // acemcp 代理命令
            crate::mcp::tools::acemcp::commands::detect_acemcp_proxy,
            crate::mcp::tools::acemcp::commands::test_acemcp_proxy_speed,

            // docs命令（迁移至 tools::docs::commands）
            crate::mcp::tools::docs::commands::test_docs_connection,
            crate::mcp::tools::docs::commands::get_docs_config,
            crate::mcp::tools::docs::commands::save_docs_config,

            // 自定义prompt命令
            get_custom_prompt_config,
            add_custom_prompt,
            update_custom_prompt,
            delete_custom_prompt,
            set_custom_prompt_enabled,
            update_custom_prompt_order,
            update_conditional_prompt_state,

            // 快捷键命令
            get_shortcut_config,
            update_shortcut_binding,
            reset_shortcuts_to_default,

            // 配置管理命令
            get_config_file_path,

            // Telegram 命令
            get_telegram_config,
            set_telegram_config,
            test_telegram_connection_cmd,
            auto_get_chat_id,
            start_telegram_sync,

            // 系统命令
            open_external_url,
            exit_app,
            handle_app_exit_request,
            force_exit_app,
            reset_exit_attempts_cmd,

            // 更新命令
            check_for_updates,
            download_and_install_update,
            get_current_version,
            restart_app,

            // 代理配置命令
            crate::network::commands::get_proxy_config,
            crate::network::commands::set_proxy_config,
            crate::network::commands::test_proxy_connection,
            crate::network::commands::detect_available_proxy
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();

            // 应用初始化
            tauri::async_runtime::block_on(async {
                if let Err(e) = setup_application(&app_handle).await {
                    log_important!(error, "应用初始化失败: {}", e);
                }
            });

            match crate::mcp::clean_expired_pantry_items(Duration::from_secs(60 * 60 * 24 * 7)) {
                Ok(deleted) => {
                    if deleted > 0 {
                        log::info!("已清理过期食材: {} 项", deleted);
                    }
                }
                Err(e) => {
                    log::warn!("清理过期食材失败: {}", e);
                }
            }

            Ok(())
        })
}

/// 运行Tauri应用
pub fn run_tauri_app() {
    build_tauri_app()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
