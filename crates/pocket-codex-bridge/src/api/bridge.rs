//! FRB-exposed bridge surface: config, discovery, API-service subscribe,
//! and app-server remote control (sessions, threads, turns, event stream).
//! Thin glue over `crate::engine`; DTOs are plain (FRB-friendly) structs.
//!
//! This file holds the persisted-config calls; each submodule owns one area
//! of the surface, and every item is re-exported here so
//! `crate::api::bridge::*` paths stay unchanged.
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use flutter_rust_bridge::frb;
use pocket_codex_core::config::Mode;

use crate::{
    engine::{
        account, app_session, claude_setup, config, discovery, logging, meta, runtime, serve,
        serve_acp,
        session_engine::{self, engine, require_native, Protocol, StartOptions, TurnOptions},
        sessions, transport,
    },
    frb_generated::StreamSink,
};

pub mod codex_setup;
pub mod connection;
pub mod device_management;
pub mod device_metadata;
pub mod events;
pub mod host_config;
pub mod host_sessions;
pub mod hosted_account;
pub mod hosting;
pub mod native_sessions;
pub mod services;
pub mod session_access;
pub mod session_config;
pub mod threads;

pub use self::{
    codex_setup::{
        codex_auth_status, codex_login_cancel, codex_login_chatgpt_start, codex_logout,
        codex_prompt_variant, codex_set_prompt_variant, codex_setup_provider, codex_setup_status,
        CodexAuthStatusDto, CodexLoginStartDto, CodexSetupStatusDto,
    },
    connection::{
        api_probe, api_probe_local, app_capabilities, app_connect, app_disconnect,
        app_is_connected, app_probe, app_probe_local, app_probe_reason, app_running_threads,
        AppCapabilitiesDto,
    },
    device_management::{
        device_management_set_enabled, device_management_status, host_restore_overrides,
        host_restore_problem, host_restore_service, managed_claude_prepare, managed_device,
        managed_host, managed_host_apply, managed_host_save, managed_host_start,
        managed_host_start_saved, managed_host_stop, managed_runtimes, DeviceManagementDto,
        HostRestoreOverrideDto, ManagedDeviceDto, ManagedHostDto, ManagedHostSaveDto,
        ManagedRuntimeAvailabilityDto, ManagedRuntimeDto,
    },
    device_metadata::{local_device_metadata, service_device_metadata, DeviceMetadataDto},
    events::{
        app_events, log_events, meta_retry_events, AppEventDto, LogLineDto, RetryProgressDto,
    },
    host_config::{
        app_serve_apply, app_serve_start_configured, codex_local_auth_status,
        codex_local_login_cancel, codex_local_login_start, codex_local_logout,
        codex_local_session_close, codex_local_session_open, host_config_get, host_config_save,
        host_config_supported, CodexLocalAuthDto, HostActiveDto, HostApplyDto, HostConfigDto,
        HostConfigInputDto, HostConfigSupportDto, HostRuntimeDto, HostStartedDto,
    },
    host_sessions::{
        app_force_resume, app_local_session_transcript, app_local_sessions, app_session_liveness,
        meta_ensure_default_project, meta_file_download, meta_file_preview, meta_force_resume,
        meta_host_is_local, meta_list_dir, meta_list_files, meta_project_config,
        meta_project_settings, meta_read_file, meta_read_thread_image, meta_save_project_settings,
        meta_session_events, meta_session_liveness, meta_session_transcript, meta_sessions,
        meta_set_project_config, meta_thread_config_get, meta_thread_config_set,
        meta_upload_context, meta_upload_file, meta_upload_file_scoped, meta_write_file,
        DirEntryDto, FileEntryDto, FilePreviewDto, ForceResumeReportDto, HolderDto,
        LocalSessionDto, ProjectConfigDto, ProjectSettingsDto, SessionFollowUpdateDto,
        SessionLivenessDto, ThreadConfigDto,
    },
    hosted_account::{
        account_current_user, account_deregister_service, account_login_poll, account_login_start,
        account_logout, account_services, account_web_login_exchange, account_web_login_start,
        AccountPollDto, AccountServiceDto, AccountUserDto, DeviceCodeDto, WebLoginStartDto,
    },
    hosting::{
        acp_claude_prepare_cancel, acp_claude_prepare_cancel_run, acp_claude_prepare_start,
        acp_claude_prepare_state, acp_claude_status, acp_hosting_supported, acp_locate,
        acp_presets, app_serve_deregister, app_serve_reregister, app_serve_restart_acp,
        app_serve_start, app_serve_start_acp, app_serve_status, app_serve_stop, app_serve_stop_all,
        codex_locate, embedded_codex_version, opencode_detect, opencode_refresh, AcpAgentSpecDto,
        AcpClaudeSetupDto, AcpClaudeStatusDto, AcpPresetDto, AcpServeDto, AppServeDto,
        AppServeStatusDto, OpenCodeEnvironmentDto, OpenCodeHostStatusDto,
    },
    native_sessions::{
        app_native_session_page, app_native_session_revision, app_native_sessions,
        NativeSessionListDto, NativeSessionPageDto, NativeSessionRevisionDto,
    },
    services::{
        api_subscribe, api_unsubscribe, discover_services, subscriptions, ServiceIdDto,
        SubStatusDto,
    },
    session_config::{
        app_session_settings, app_set_session_config, app_set_session_mode, SessionConfigOptionDto,
        SessionConfigValueDto, SessionModeDto, SessionSettingsDto,
    },
    threads::{
        app_compact, app_git_diff, app_history_cached, app_history_focus, app_history_prefetch,
        app_history_sync_prepare, app_model_list, app_rate_limits, app_realtime_request,
        app_respond_approval, app_respond_user_input, app_set_thread_name, app_thread_list,
        app_thread_metadata, app_thread_older_page, app_thread_read, app_thread_resume,
        app_thread_runtime_config, app_thread_start, app_thread_summary, app_thread_turn_items,
        app_thread_turn_page, app_turn_interrupt, app_turn_start, app_turn_steer,
        app_turn_supplement, history_cache_set_limit, history_cache_status, AppTurnSupplementDto,
        HistoryCacheStatusDto, ModelInfoDto, OlderPageDto, ThreadHistoryDto, ThreadItemDto,
        ThreadMetaDto, ThreadRuntimeConfigDto, TurnItemsPageDto, TurnSummaryDto,
    },
};

/// View of persisted config for the UI; never exposes the raw key or token.
pub struct ConfigView {
    /// Configured relay `host:port`, if any.
    pub relay: Option<String>,
    /// Whether a 32-byte key is stored (value withheld).
    pub has_key: bool,
    /// Configured UI locale (BCP-47, e.g. `en`/`zh`), or `None` to follow
    /// the system locale.
    pub locale: Option<String>,
    /// Active transport mode: `account` / `self_host` / `unconfigured`.
    pub mode: String,
    /// Signed-in GitHub login (account mode), if any.
    pub account_login: Option<String>,
    /// Signed-in GitHub numeric account id, if any. The UI builds the avatar
    /// URL from it; there is no avatar field to fetch.
    pub account_id: Option<String>,
    /// Whether an account session token is stored (value withheld).
    pub has_account_token: bool,
}

/// Initialise the engine with the platform app-support dir (from Dart's
/// path_provider). Must be called once after `RustLib.init()`.
pub fn init_bridge(support_dir: String) -> Result<()> {
    // Pin the process crypto provider before any TLS connection is opened.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Capture bridge events before starting local services.
    logging::init();
    let support_dir = PathBuf::from(support_dir);
    // Mirror the captured log to disk (last 6 hours), so a hang can be read
    // after the fact instead of only in a viewer that was open at the time.
    logging::init_file(&support_dir);
    runtime::init(support_dir)?;
    crate::engine::management::initialize();
    Ok(())
}

/// Current config view (relay/key presence, locale, and account state).
pub fn get_config() -> Result<ConfigView> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let mode = match cfg.account_mode() {
        Mode::Account => "account",
        Mode::SelfHost => "self_host",
        Mode::Unconfigured => "unconfigured",
    }
    .to_string();
    Ok(ConfigView {
        relay: cfg.relay().map(str::to_string),
        has_key: cfg.relay_key().is_some(),
        locale: cfg.locale().map(str::to_string),
        mode,
        account_login: cfg.account_login().map(str::to_string),
        account_id: cfg.account_id().map(str::to_string),
        has_account_token: cfg.account_token().is_some(),
    })
}

/// Set the relay `host:port` and persist.
pub fn set_relay(relay: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_relay(&relay);
        Ok(())
    })?;
    // Connections of the previous relay are not this relay's services.
    transport::context_changed();
    Ok(())
}

/// Set the 32-byte MSG_HEADER_KEY and persist (validates length).
pub fn set_key(key: String) -> Result<()> {
    if key.len() != 32 {
        return Err(anyhow!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", key.len()));
    }
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_relay_key(&key);
        Ok(())
    })?;
    transport::context_changed();
    Ok(())
}

/// Set the UI locale (BCP-47, e.g. `en`/`zh`) and persist. An empty string
/// clears it, meaning the app follows the system locale.
pub fn set_locale(locale: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_locale(&locale);
        Ok(())
    })
}

/// Import a `pcx1:` share string: decode, persist relay + key, return relay.
pub fn import_config(text: String) -> Result<String> {
    let payload = config::decode_pcx1(&text)?;
    let dir = runtime::support_dir()?;
    config::update_config(&dir, |cfg| {
        cfg.set_relay(&payload.relay);
        cfg.set_relay_key(&payload.key);
        Ok(())
    })?;
    transport::context_changed();
    Ok(payload.relay)
}

/// Export the current relay+key as a `pcx1:` share string.
pub fn export_config() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let relay = cfg.relay().ok_or_else(|| anyhow!("no relay configured"))?;
    let key = cfg
        .relay_key()
        .ok_or_else(|| anyhow!("no key configured"))?;
    config::encode_pcx1(relay, key)
}
