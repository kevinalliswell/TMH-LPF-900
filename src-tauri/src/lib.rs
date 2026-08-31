//! Thin Tauri glue (docs/03). Owns the M2 `DeviceHub` in managed state, exposes it
//! to the webview through typed commands + a telemetry Channel, and hardens the shell
//! (single-instance). M4 runs the real experiment engine: a per-run tick task drives
//! the furnace through its program stages, applies each stage's atmosphere through the
//! safety clamp, samples to SQLite, and guarantees the safety atmosphere on every exit.

mod auth_service;
mod auth_session;

use auth_service::{AuthCode, AuthService, AuthSnapshot, PublicUser};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use tmh_core::audit::{AuditEntry, AuditLog, AuditResult, Category};
use tmh_core::auth::{hash_password, validate_display_name, UserRole};
use tmh_core::bus::{BalanceActor, DeviceHub, HubIntervals, MfcActor, TempActor};
use tmh_core::clamp::ClampResult;
use tmh_core::config::CommConfig;
use tmh_core::engine::{
    standard_program, ExperimentEngine, Phase, Program, Stage, StageKind, SAFETY_ATMOSPHERE,
};
use tmh_core::gb::{free_swelling, rdi, reducibility, reduction_rate_index};
use tmh_core::sampler::{build_sample_row, SampleContext};
use tmh_core::storage::{ExperimentRow, SampleRow, Storage, UserRow};
use tmh_core::telemetry::{aggregate, TelemetrySnapshot};
use tmh_core::transport::MockTransport;

/// Registered experiment modes (custom modes land in a later slice).
const MODES: [&str; 3] = ["REDUCIBILITY", "LOW_TEMP_DEGRADATION", "FREE_SWELLING"];
/// Number of gas channels (order: H2, N2, CO2, CO).
const GAS_COUNT: usize = 4;
/// Mock reduction removes this fraction of the sample mass (so GB analysis has signal).
const MOCK_REDUCTION_LOSS: f64 = 0.12;
/// Flush buffered samples to SQLite every N ticks.
const FLUSH_EVERY_TICKS: u32 = 5;

/// Lock a mutex tolerating poisoning: a panic while the lock was held must not turn every
/// later command into a panic (our guarded data stays consistent across such panics) (#14).
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Managed application state.
struct AppState {
    hub: Arc<DeviceHub>,
    mode: String,
    limit_h2: f64,
    limit_co: f64,
    audit: Arc<AuditLog>,
    storage: Arc<Mutex<Storage>>,
    auth: Arc<AuthService>,
    exp: Arc<ExpControl>,
    /// Simulated-seconds advanced per real second (mock time acceleration; 1.0 = real).
    sim_speed: f64,
    /// Directory reports and data exports are written to.
    exports_dir: PathBuf,
    /// Software-update checking (docs/09 §9.3): OFF by default, enabled via config/env.
    updater_enabled: bool,
    /// Bumped on each (un)subscribe so a stale telemetry pump thread exits (issue #8).
    telemetry_gen: Arc<AtomicU64>,
}

/// Shared control block for the single running experiment. `status` is the published
/// snapshot the UI polls/receives; `stop`/`skip` are one-shot signals the tick task
/// observes on its next cycle.
struct ExpControl {
    status: Mutex<ExperimentStatus>,
    stop: AtomicBool,
    skip: AtomicBool,
}

impl ExpControl {
    fn new() -> Self {
        Self {
            status: Mutex::new(ExperimentStatus::idle()),
            stop: AtomicBool::new(false),
            skip: AtomicBool::new(false),
        }
    }
    fn snapshot(&self) -> ExperimentStatus {
        lock(&self.status).clone()
    }
    fn is_running(&self) -> bool {
        lock(&self.status).running
    }
    fn set_status(&self, s: ExperimentStatus) {
        *lock(&self.status) = s;
    }
}

/// The experiment snapshot published to the UI (serde snake_case, matching the M3 TS
/// bindings; `phase`/`stage_kind` render as SCREAMING_SNAKE_CASE via their own attrs).
#[derive(Clone, serde::Serialize)]
struct ExperimentStatus {
    running: bool,
    experiment_id: Option<String>,
    mode: Option<String>,
    sample_name: Option<String>,
    started_at: Option<String>,
    phase: Phase,
    stage_index: usize,
    total_stages: usize,
    stage_name: Option<String>,
    stage_kind: Option<StageKind>,
    target_temp: f64,
    tolerance: f64,
    stage_elapsed_s: f64,
    total_elapsed_s: f64,
    owner_user_id: Option<String>,
    owner_display_name: Option<String>,
    can_control: bool,
    occupied_by_other: bool,
}

impl ExperimentStatus {
    fn idle() -> Self {
        Self {
            running: false,
            experiment_id: None,
            mode: None,
            sample_name: None,
            started_at: None,
            phase: Phase::Idle,
            stage_index: 0,
            total_stages: 0,
            stage_name: None,
            stage_kind: None,
            target_temp: 0.0,
            tolerance: 0.0,
            stage_elapsed_s: 0.0,
            total_elapsed_s: 0.0,
            owner_user_id: None,
            owner_display_name: None,
            can_control: false,
            occupied_by_other: false,
        }
    }

    fn safety_error(experiment_id: &str, total_elapsed_s: f64) -> Self {
        Self {
            experiment_id: Some(experiment_id.to_string()),
            phase: Phase::Error,
            total_elapsed_s,
            ..Self::idle()
        }
    }
}

#[derive(serde::Deserialize)]
struct ExperimentParams {
    sample_id: String,
    sample_name: String,
    sample_weight: f64,
    experiment_type: String,
    project_name: Option<String>,
    notes: Option<String>,
}

fn iso_now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn epoch_now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn auth_error(code: AuthCode) -> String {
    code.to_string()
}

fn current_user(state: &AppState) -> Result<UserRow, String> {
    let storage = lock(&state.storage);
    state
        .auth
        .require_user(&storage, epoch_now())
        .map_err(auth_error)
}

fn current_admin(state: &AppState) -> Result<UserRow, String> {
    let storage = lock(&state.storage);
    state
        .auth
        .require_admin(&storage, epoch_now())
        .map_err(auth_error)
}

fn authorize_experiment(
    state: &AppState,
    id: &str,
    user: &UserRow,
) -> Result<ExperimentRow, String> {
    let row = lock(&state.storage)
        .get_experiment(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "实验不存在".to_string())?;
    if user.role.is_admin() || row.owner_user_id.as_deref() == Some(user.user_id.as_str()) {
        Ok(row)
    } else {
        Err(AuthCode::Forbidden.to_string())
    }
}

fn require_experiment_controller(state: &AppState) -> Result<UserRow, String> {
    let user = current_user(state)?;
    let status = state.exp.snapshot();
    if !status.running
        || user.role.is_admin()
        || status.owner_user_id.as_deref() == Some(user.user_id.as_str())
    {
        Ok(user)
    } else {
        Err(AuthCode::Forbidden.to_string())
    }
}

/// Phase → stable string for the `experiment_status` sample column.
fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Idle => "IDLE",
        Phase::Configuring => "CONFIGURING",
        Phase::Running => "RUNNING",
        Phase::StageTransition => "STAGE_TRANSITION",
        Phase::Completing => "COMPLETING",
        Phase::Stopping => "STOPPING",
        Phase::Error => "ERROR",
    }
}

/// Stage kind → sample status tag (lets the report isolate the reduction stage).
fn kind_str(k: StageKind) -> &'static str {
    match k {
        StageKind::Heating => "HEATING",
        StageKind::Stabilizing => "STABILIZING",
        StageKind::Reducing => "REDUCING",
        StageKind::Cooling => "COOLING",
    }
}

/// Simulated-time acceleration: `TMH_SIM_SPEED` if set, else fast for mock, real (1.0)
/// for hardware. Mock runs at 180× so a multi-hour program plays out in a few minutes.
fn sim_speed(mode: &str) -> f64 {
    std::env::var("TMH_SIM_SPEED")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(if mode == "mock" { 180.0 } else { 1.0 })
}

/// Compiled-in default comm config (the repo's config/comm_config.json), used when no
/// external config file is present or it fails to load. Kept in sync with docs/06 §6.4.
const DEFAULT_COMM_CONFIG: &str = include_str!("../../config/comm_config.json");

/// Load + validate the comm config (docs/06 §6.4) so the safety limits, MFC slave
/// addresses and flow scaling come from the file (issue #11). Resolution order:
/// `TMH_COMM_CONFIG` env override, else `config/comm_config.json` in the working dir;
/// on any failure fall back to the compiled-in default so the app still runs (mock).
fn load_comm_config() -> CommConfig {
    let path = std::env::var("TMH_COMM_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config/comm_config.json"));
    match CommConfig::load(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "comm config unavailable/invalid ({}: {e}); using built-in defaults",
                path.display()
            );
            CommConfig::from_json(DEFAULT_COMM_CONFIG)
                .expect("built-in default comm config must parse")
        }
    }
}

fn build_mock_hub(cfg: &CommConfig) -> DeviceHub {
    // Idle furnace sits at ambient; a running experiment drives the target per stage. The
    // mock actors are driven by the config so edits to limits/scaling/addresses take effect
    // (the safety clamp is still backed by its compile-time ceiling).
    let temp = TempActor::new(
        Box::new(MockTransport::temp()),
        cfg.temp.slave_address,
        cfg.temp.scale,
        cfg.temp.signed_registers,
    );
    let mfc = MfcActor::from_config(Box::new(MockTransport::mfc()), cfg);
    let balance = BalanceActor::new(Box::new(MockTransport::balance(100.0)));
    DeviceHub::spawn(temp, mfc, balance, HubIntervals::default())
}

// ---- Engine wiring helpers --------------------------------------------------

/// Apply a stage's atmosphere through the clamp and steer the mock furnace/balance.
/// The MFC bus actor clamps H2/CO at frame-build time — this is not an enforcement point.
fn apply_stage_atmosphere(hub: &DeviceHub, stage: &Stage, w0: f64) -> Result<(), String> {
    hub.request_set_atmosphere(stage.gas)
        .blocking_recv()
        .map_err(|_| "DEVICE_OFFLINE".to_string())?
        .map_err(|_| "DEVICE_OFFLINE".to_string())?;
    hub.set_temp_target_c(stage.target_temp);
    if stage.kind == StageKind::Reducing {
        // Mock only: reduction strips oxygen, so the sample loses mass over the stage.
        hub.set_balance_target_g(w0 * (1.0 - MOCK_REDUCTION_LOSS));
    }
    Ok(())
}

/// The safety atmosphere (docs/05 §5.5): N2=5.0, H2=CO2=CO=0. Applied on EVERY exit.
fn apply_safety_atmosphere(hub: &DeviceHub) -> Result<(), String> {
    let result = hub
        .request_set_atmosphere(SAFETY_ATMOSPHERE)
        .blocking_recv()
        .map_err(|_| "DEVICE_OFFLINE".to_string())?
        .map_err(|_| "DEVICE_OFFLINE".to_string())
        .map(|_| ());
    hub.set_temp_target_c(25.0);
    result
}

/// Like [`apply_safety_atmosphere`] but BLOCKS until the MFC bus thread confirms each
/// setpoint. Used on application exit (issue #1) so the process never terminates with
/// flammable gas still commanded. Must run on a non-async thread (the Tauri run-event
/// loop) — `blocking_recv` panics inside a tokio runtime.
fn apply_safety_atmosphere_blocking(hub: &DeviceHub) -> Result<(), String> {
    apply_safety_atmosphere(hub)
}

/// Snapshot one sample row from live telemetry. Delegates to the shared core sampler so
/// the balance-offline annotation, weight=0 rule, PV semantics and grams weight-loss unit
/// live in exactly one place (issue #7). `status` tags the stage (e.g. REDUCING).
fn build_sample(hub: &DeviceHub, id: &str, w0: f64, total_s: f64, status: &str) -> SampleRow {
    let temp = hub.temp_rx().borrow().clone();
    let mfc = hub.mfc_rx().borrow().clone();
    let balance = hub.balance_rx().borrow().clone();
    let snap = aggregate(0, &temp, &mfc, &balance);
    let ctx = SampleContext {
        experiment_id: id.to_string(),
        initial_weight: w0,
    };
    build_sample_row(&ctx, &snap, iso_now(), total_s.max(0.0) as u64, status)
}

/// Build the running-status snapshot from the engine's current position.
fn status_running(
    engine: &ExperimentEngine,
    id: &str,
    mode: &str,
    sample: &str,
    started: &str,
    owner_user_id: &str,
    owner_display_name: &str,
) -> ExperimentStatus {
    let stage = engine.current_stage();
    ExperimentStatus {
        running: true,
        experiment_id: Some(id.to_string()),
        mode: Some(mode.to_string()),
        sample_name: Some(sample.to_string()),
        started_at: Some(started.to_string()),
        phase: engine.phase(),
        stage_index: engine.stage_index(),
        total_stages: engine.total_stages(),
        stage_name: stage.map(|s| s.name.clone()),
        stage_kind: stage.map(|s| s.kind),
        target_temp: stage.map(|s| s.target_temp).unwrap_or(0.0),
        tolerance: stage.map(|s| s.tolerance).unwrap_or(0.0),
        stage_elapsed_s: engine.stage_elapsed_s(),
        total_elapsed_s: engine.total_elapsed_s(),
        owner_user_id: Some(owner_user_id.to_string()),
        owner_display_name: Some(owner_display_name.to_string()),
        can_control: true,
        occupied_by_other: false,
    }
}

/// Flush buffered samples in one transaction. On error the buffer is retained for retry;
/// sampling must never take down the control loop.
fn flush(storage: &Arc<Mutex<Storage>>, buffer: &mut Vec<SampleRow>) {
    if buffer.is_empty() {
        return;
    }
    if let Ok(mut s) = storage.lock() {
        if s.insert_samples(buffer).is_ok() {
            buffer.clear();
        }
    }
}

/// Publish + emit the current running status.
#[allow(clippy::too_many_arguments)]
fn publish_running(
    app: &AppHandle,
    exp: &ExpControl,
    engine: &ExperimentEngine,
    id: &str,
    mode: &str,
    sample: &str,
    started: &str,
    owner_user_id: &str,
    owner_display_name: &str,
) {
    let status = status_running(
        engine,
        id,
        mode,
        sample,
        started,
        owner_user_id,
        owner_display_name,
    );
    exp.set_status(status.clone());
    let _ = app.emit("experiment-state-changed", status);
}

/// End-of-run bookkeeping shared by completion and stop: safety atmosphere, final flush,
/// DB close-out, audit, and publish idle. Idempotent.
#[allow(clippy::too_many_arguments)]
fn finalize(
    app: &AppHandle,
    hub: &DeviceHub,
    storage: &Arc<Mutex<Storage>>,
    audit: &AuditLog,
    exp: &ExpControl,
    buffer: &mut Vec<SampleRow>,
    id: &str,
    outcome: &str,
    total_s: f64,
) {
    let safety_result = apply_safety_atmosphere(hub); // hard invariant — every exit path
    flush(storage, buffer);
    let end = iso_now();
    if let Ok(s) = storage.lock() {
        let _ = s.finish_experiment(id, &end);
    }
    let safety_ok = safety_result.is_ok();
    audit.write(
        &AuditEntry::new(
            Category::Experiment,
            "safety_atmosphere",
            if safety_ok {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .detail(serde_json::json!({ "id": id, "error": safety_result.err() })),
    );
    audit.write(
        &AuditEntry::new(
            Category::Experiment,
            outcome,
            if safety_ok {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .detail(serde_json::json!({
            "id": id,
            "elapsed_s": total_s,
            "safety_atmosphere_confirmed": safety_ok
        })),
    );
    let final_status = if safety_ok {
        ExperimentStatus::idle()
    } else {
        ExperimentStatus::safety_error(id, total_s)
    };
    exp.set_status(final_status.clone());
    let _ = app.emit("experiment-state-changed", final_status);
}

/// Spawn the per-run control loop: tick the engine once per real second (advancing
/// `sim_speed` simulated seconds), (re)apply stage atmospheres, sample to SQLite, and
/// finalize on completion or stop.
#[allow(clippy::too_many_arguments)]
fn spawn_experiment_task(
    app: AppHandle,
    hub: Arc<DeviceHub>,
    storage: Arc<Mutex<Storage>>,
    audit: Arc<AuditLog>,
    exp: Arc<ExpControl>,
    program: Program,
    id: String,
    mode: String,
    sample: String,
    w0: f64,
    started: String,
    owner_user_id: String,
    owner_display_name: String,
    sim_speed: f64,
) {
    std::thread::spawn(move || {
        let mut engine = ExperimentEngine::start(program);
        let mut buffer: Vec<SampleRow> = Vec::new();
        if let Some(s) = engine.current_stage() {
            if apply_stage_atmosphere(&hub, s, w0).is_err() {
                audit.write(
                    &AuditEntry::new(
                        Category::Experiment,
                        "stage_atmosphere",
                        AuditResult::Failure,
                    )
                    .detail(serde_json::json!({ "id": id, "stage": 0 })),
                );
                finalize(
                    &app,
                    &hub,
                    &storage,
                    &audit,
                    &exp,
                    &mut buffer,
                    &id,
                    "error",
                    0.0,
                );
                return;
            }
        }
        publish_running(
            &app,
            &exp,
            &engine,
            &id,
            &mode,
            &sample,
            &started,
            &owner_user_id,
            &owner_display_name,
        );

        let mut ticks_since_flush = 0u32;

        loop {
            std::thread::sleep(Duration::from_secs(1));

            if exp.stop.load(Ordering::Relaxed) {
                finalize(
                    &app,
                    &hub,
                    &storage,
                    &audit,
                    &exp,
                    &mut buffer,
                    &id,
                    "stopped",
                    engine.total_elapsed_s(),
                );
                break;
            }

            // Safety (issue #3): sustained loss of a safety-critical bus (5 consecutive
            // failures marks it offline) is an error-exit path — force the protective
            // atmosphere instead of running blind with flammable gas open.
            let mfc_ok = hub.mfc_rx().borrow().connected;
            let temp_ok = hub.temp_rx().borrow().connected;
            if !mfc_ok || !temp_ok {
                let device = if !mfc_ok { "mfc" } else { "temp" };
                audit.write(
                    &AuditEntry::new(
                        Category::Experiment,
                        "device_offline_abort",
                        AuditResult::Failure,
                    )
                    .detail(serde_json::json!({ "id": id, "device": device })),
                );
                finalize(
                    &app,
                    &hub,
                    &storage,
                    &audit,
                    &exp,
                    &mut buffer,
                    &id,
                    "error",
                    engine.total_elapsed_s(),
                );
                break;
            }

            let t8 = hub.temp_rx().borrow().temps[7].unwrap_or(25.0);
            let result = if exp.skip.swap(false, Ordering::Relaxed) {
                audit.write(
                    &AuditEntry::new(Category::Experiment, "skip_stage", AuditResult::Success)
                        .detail(
                            serde_json::json!({ "id": id, "from_stage": engine.stage_index() }),
                        ),
                );
                engine.skip_stage()
            } else {
                engine.tick(t8, sim_speed)
            };

            if result.apply_stage.is_some() && !exp.stop.load(Ordering::Relaxed) {
                if let Some(s) = engine.current_stage() {
                    if apply_stage_atmosphere(&hub, s, w0).is_err() {
                        audit.write(
                            &AuditEntry::new(
                                Category::Experiment,
                                "stage_atmosphere",
                                AuditResult::Failure,
                            )
                            .detail(serde_json::json!({
                                "id": id, "stage": engine.stage_index(), "name": s.name
                            })),
                        );
                        finalize(
                            &app,
                            &hub,
                            &storage,
                            &audit,
                            &exp,
                            &mut buffer,
                            &id,
                            "error",
                            engine.total_elapsed_s(),
                        );
                        break;
                    }
                    audit.write(
                        &AuditEntry::new(Category::Experiment, "stage_enter", AuditResult::Success)
                            .detail(serde_json::json!({
                                "id": id, "stage": engine.stage_index(), "name": s.name
                            })),
                    );
                }
                // Close the stop×stage-atmosphere TOCTOU (issue #5): if a stop landed while
                // we were applying the stage atmosphere, re-assert the safety atmosphere now.
                // The MFC thread processes commands FIFO, so this last write wins over the
                // stage gas and flammables are never left open after a stop.
                if exp.stop.load(Ordering::Relaxed) {
                    let _ = apply_safety_atmosphere(&hub);
                }
            }

            let status = engine
                .current_stage()
                .map(|s| kind_str(s.kind))
                .unwrap_or_else(|| phase_str(engine.phase()));
            buffer.push(build_sample(
                &hub,
                &id,
                w0,
                engine.total_elapsed_s(),
                status,
            ));
            ticks_since_flush += 1;
            if ticks_since_flush >= FLUSH_EVERY_TICKS {
                flush(&storage, &mut buffer);
                ticks_since_flush = 0;
            }

            if result.completed {
                finalize(
                    &app,
                    &hub,
                    &storage,
                    &audit,
                    &exp,
                    &mut buffer,
                    &id,
                    "completed",
                    engine.total_elapsed_s(),
                );
                break;
            }

            publish_running(
                &app,
                &exp,
                &engine,
                &id,
                &mode,
                &sample,
                &started,
                &owner_user_id,
                &owner_display_name,
            );
        }
    });
}

/// Startup crash recovery (docs/05 §5.5): any experiment left open (no `end_time`) by a
/// crash gets the safety atmosphere and is closed out before the UI comes up.
fn recover_unfinished(hub: &DeviceHub, storage: &Storage, audit: &AuditLog) {
    let unfinished = storage.unfinished_experiments().unwrap_or_default();
    if unfinished.is_empty() {
        return;
    }
    if let Err(error) = apply_safety_atmosphere(hub) {
        audit.write(
            &AuditEntry::new(Category::Experiment, "crash_recovery", AuditResult::Failure)
                .detail(serde_json::json!({ "error": error, "recovered": [] })),
        );
        return;
    }
    let now = iso_now();
    let ids: Vec<String> = unfinished.iter().map(|e| e.experiment_id.clone()).collect();
    for id in &ids {
        let _ = storage.finish_experiment(id, &now);
    }
    audit.write(
        &AuditEntry::new(Category::Experiment, "crash_recovery", AuditResult::Success)
            .detail(serde_json::json!({ "recovered": ids })),
    );
}

// ---- DTOs -------------------------------------------------------------------

#[derive(serde::Serialize)]
struct AppInfo {
    name: String,
    version: String,
}

#[derive(serde::Serialize)]
struct GasLimits {
    h2: f64,
    co: f64,
}

#[derive(serde::Serialize)]
struct HealthDot {
    connected: bool,
    consecutive_errors: u32,
}

#[derive(serde::Serialize)]
struct DeviceHealthDto {
    temp: HealthDot,
    mfc: HealthDot,
    balance: HealthDot,
}

#[derive(serde::Serialize)]
struct TareResultDto {
    success: bool,
    a00_found: bool,
    weight: Option<f64>,
}

#[derive(serde::Serialize)]
struct PublicSafetyStatus {
    experiment_running: bool,
    mfc_connected: bool,
    temp_connected: bool,
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
fn get_auth_state(state: State<AppState>) -> Result<AuthSnapshot, String> {
    state
        .auth
        .snapshot(&lock(&state.storage), epoch_now())
        .map_err(auth_error)
}

#[tauri::command]
fn bootstrap_admin(
    state: State<AppState>,
    username: String,
    display_name: String,
    password: String,
) -> Result<PublicUser, String> {
    let user = state
        .auth
        .bootstrap(
            &lock(&state.storage),
            &username,
            &display_name,
            &password,
            epoch_now(),
        )
        .map_err(auth_error)?;
    state.audit.write(
        &AuditEntry::new(Category::App, "bootstrap_admin", AuditResult::Success)
            .operator(user.display_name.clone())
            .detail(serde_json::json!({ "user_id": user.user_id })),
    );
    Ok(PublicUser::from(&user))
}

#[tauri::command]
fn login(
    state: State<AppState>,
    username: String,
    password: String,
) -> Result<AuthSnapshot, String> {
    let result = state
        .auth
        .login(&lock(&state.storage), &username, &password, epoch_now());
    state.audit.write(
        &AuditEntry::new(
            Category::App,
            "login",
            if result.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Rejected
            },
        )
        .detail(serde_json::json!({ "username": username })),
    );
    result.map_err(auth_error)?;
    get_auth_state(state)
}

#[tauri::command]
fn logout(state: State<AppState>) {
    state.auth.logout();
    state.telemetry_gen.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
fn lock_session(state: State<AppState>) {
    state.auth.lock();
    state.telemetry_gen.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
fn touch_session(state: State<AppState>) -> Result<(), String> {
    state.auth.touch(epoch_now()).map_err(auth_error)
}

#[tauri::command]
fn change_own_password(
    state: State<AppState>,
    current_password: String,
    new_password: String,
) -> Result<(), String> {
    state
        .auth
        .change_own_password(
            &lock(&state.storage),
            &current_password,
            &new_password,
            epoch_now(),
        )
        .map_err(auth_error)
}

#[tauri::command]
fn list_users(state: State<AppState>) -> Result<Vec<PublicUser>, String> {
    current_admin(&state)?;
    let users = lock(&state.storage)
        .list_users()
        .map_err(|e| e.to_string())?;
    Ok(users.iter().map(PublicUser::from).collect())
}

#[tauri::command]
fn create_user(
    state: State<AppState>,
    username: String,
    display_name: String,
    temporary_password: String,
    role: UserRole,
) -> Result<PublicUser, String> {
    let user = state
        .auth
        .create_user(
            &lock(&state.storage),
            &username,
            &display_name,
            &temporary_password,
            role,
            epoch_now(),
        )
        .map_err(auth_error)?;
    Ok(PublicUser::from(&user))
}

#[tauri::command]
fn update_user(
    state: State<AppState>,
    user_id: String,
    display_name: String,
    role: UserRole,
    is_active: bool,
) -> Result<PublicUser, String> {
    let actor = current_admin(&state)?;
    let display_name =
        validate_display_name(&display_name).map_err(|_| AuthCode::InvalidInput.to_string())?;
    let storage = lock(&state.storage);
    let target = storage
        .get_user(&user_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "USER_NOT_FOUND".to_string())?;
    if target.user_id == actor.user_id && (role != target.role || !is_active) {
        return Err(AuthCode::Forbidden.to_string());
    }
    if target.role.is_admin()
        && target.is_active
        && (!role.is_admin() || !is_active)
        && storage.active_admin_count().map_err(|e| e.to_string())? <= 1
    {
        return Err(AuthCode::LastAdmin.to_string());
    }
    storage
        .update_user(&user_id, &display_name, role, is_active, &iso_now())
        .map_err(|e| e.to_string())?;
    let updated = storage
        .get_user(&user_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "USER_NOT_FOUND".to_string())?;
    Ok(PublicUser::from(&updated))
}

#[tauri::command]
fn reset_user_password(
    state: State<AppState>,
    user_id: String,
    temporary_password: String,
) -> Result<(), String> {
    let actor = current_admin(&state)?;
    if actor.user_id == user_id {
        return Err(AuthCode::Forbidden.to_string());
    }
    let hash =
        hash_password(&temporary_password).map_err(|_| AuthCode::InvalidInput.to_string())?;
    let storage = lock(&state.storage);
    if storage
        .get_user(&user_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err("USER_NOT_FOUND".into());
    }
    storage
        .set_user_password(&user_id, &hash, true, &iso_now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_experiment_owner(
    state: State<AppState>,
    experiment_id: String,
    owner_user_id: Option<String>,
) -> Result<(), String> {
    current_admin(&state)?;
    if state.exp.snapshot().experiment_id.as_deref() == Some(experiment_id.as_str()) {
        return Err("无法变更正在运行实验的归属".into());
    }
    let storage = lock(&state.storage);
    if storage
        .get_experiment(&experiment_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err("实验不存在".into());
    }
    if let Some(owner_id) = owner_user_id.as_deref() {
        let owner = storage
            .get_user(owner_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "USER_NOT_FOUND".to_string())?;
        if !owner.is_active {
            return Err(AuthCode::AccountDisabled.to_string());
        }
    }
    storage
        .set_experiment_owner(&experiment_id, owner_user_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        name: "TMH-LPF-900".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[tauri::command]
fn get_runtime_mode(state: State<AppState>) -> String {
    state.mode.clone()
}

#[tauri::command]
fn get_gas_safety_limits(state: State<AppState>) -> GasLimits {
    GasLimits {
        h2: state.limit_h2,
        co: state.limit_co,
    }
}

#[tauri::command]
fn get_device_health(state: State<AppState>) -> DeviceHealthDto {
    let t = state.hub.temp_rx().borrow().clone();
    let m = state.hub.mfc_rx().borrow().clone();
    let b = state.hub.balance_rx().borrow().clone();
    DeviceHealthDto {
        temp: HealthDot {
            connected: t.connected,
            consecutive_errors: t.consecutive_errors,
        },
        mfc: HealthDot {
            connected: m.connected,
            consecutive_errors: m.consecutive_errors,
        },
        balance: HealthDot {
            connected: b.connected,
            consecutive_errors: b.consecutive_errors,
        },
    }
}

#[tauri::command]
fn get_public_safety_status(state: State<AppState>) -> PublicSafetyStatus {
    PublicSafetyStatus {
        experiment_running: state.exp.is_running(),
        mfc_connected: state.hub.mfc_rx().borrow().connected,
        temp_connected: state.hub.temp_rx().borrow().connected,
    }
}

#[tauri::command]
async fn emergency_safe_shutdown(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    st.exp.stop.store(true, Ordering::Relaxed);
    let result = match st.hub.request_set_atmosphere(SAFETY_ATMOSPHERE).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) | Err(_) => Err("DEVICE_OFFLINE".to_string()),
    };
    st.hub.set_temp_target_c(25.0);
    st.audit.write(
        &AuditEntry::new(
            Category::Experiment,
            "emergency_safe_shutdown",
            if result.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .detail(serde_json::json!({ "authenticated": false })),
    );
    result
}

#[tauri::command]
async fn set_gas_flow(app: AppHandle, gas: usize, value: f64) -> Result<ClampResult, String> {
    require_experiment_controller(&app.state::<AppState>())?;
    // Reject out-of-range channels from the untrusted webview (defense in depth; the MFC
    // actor also bounds-guards). There are exactly 4 gas channels (H2, N2, CO2, CO).
    if gas >= GAS_COUNT {
        return Err("无效的气体通道".into());
    }
    let hub = app.state::<AppState>().hub.clone();
    let result = match hub.request_set_flow(gas, value).await {
        Ok(result) => result,
        Err(_) => {
            app.state::<AppState>().audit.write(
                &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Failure).detail(
                    serde_json::json!({ "gas": gas, "requested": value, "error": "actor_unavailable" }),
                ),
            );
            return Err("DEVICE_OFFLINE".into());
        }
    };
    let st = app.state::<AppState>();
    let clamp = match result {
        Ok(clamp) => clamp,
        Err(error) => {
            st.audit.write(
                &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Failure).detail(
                    serde_json::json!({ "gas": gas, "requested": value, "error": error.to_string() }),
                ),
            );
            return Err("DEVICE_OFFLINE".into());
        }
    };
    st.audit.write(
        &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Success).detail(
            serde_json::json!({
                "gas": gas, "requested": value, "applied": clamp.applied, "clamped": clamp.clamped
            }),
        ),
    );
    if clamp.clamped {
        let _ = app.emit(
            "safety-clamp-triggered",
            serde_json::json!({ "gas": gas, "requested": value, "applied": clamp.applied, "limit": clamp.limit }),
        );
    }
    Ok(clamp)
}

#[tauri::command]
async fn set_gas_on_off(app: AppHandle, gas: usize, on: bool) -> Result<(), String> {
    require_experiment_controller(&app.state::<AppState>())?;
    if gas >= GAS_COUNT {
        return Err("INVALID_PARAMS".into());
    }
    let hub = app.state::<AppState>().hub.clone();
    let result = match hub.request_set_on_off(gas, on).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err("DEVICE_OFFLINE".to_string()),
    };
    app.state::<AppState>().audit.write(
        &AuditEntry::new(
            Category::Gas,
            "on_off",
            if result.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .detail(serde_json::json!({ "gas": gas, "on": on })),
    );
    result
}

async fn set_all_valves(app: &AppHandle, on: bool, action: &str) -> Result<(), String> {
    let hub = app.state::<AppState>().hub.clone();
    let mut replies = Vec::with_capacity(GAS_COUNT);
    for g in 0..GAS_COUNT {
        replies.push(hub.request_set_on_off(g, on));
    }
    let mut failed = false;
    for reply in replies {
        if !matches!(reply.await, Ok(Ok(()))) {
            failed = true;
        }
    }
    app.state::<AppState>().audit.write(&AuditEntry::new(
        Category::Gas,
        action,
        if failed {
            AuditResult::Failure
        } else {
            AuditResult::Success
        },
    ));
    if failed {
        Err("DEVICE_OFFLINE".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
async fn open_all_valves(app: AppHandle) -> Result<(), String> {
    require_experiment_controller(&app.state::<AppState>())?;
    set_all_valves(&app, true, "open_all").await
}

#[tauri::command]
async fn close_all_valves(app: AppHandle) -> Result<(), String> {
    set_all_valves(&app, false, "close_all").await
}

#[tauri::command]
async fn shutoff_flammable_gases(app: AppHandle) -> Result<(), String> {
    let hub = app.state::<AppState>().hub.clone();
    let result = match hub.request_shutoff_flammable().await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err("DEVICE_OFFLINE".to_string()),
    };
    app.state::<AppState>().audit.write(&AuditEntry::new(
        Category::Gas,
        "shutoff",
        if result.is_ok() {
            AuditResult::Success
        } else {
            AuditResult::Failure
        },
    ));
    result
}

#[tauri::command]
async fn tare_balance(app: AppHandle) -> Result<TareResultDto, String> {
    require_experiment_controller(&app.state::<AppState>())?;
    let hub = app.state::<AppState>().hub.clone();
    let r = hub
        .request_tare()
        .await
        .map_err(|_| "天平不可用".to_string())?;
    app.state::<AppState>().audit.write(&AuditEntry::new(
        Category::Balance,
        "tare",
        if r.success {
            AuditResult::Success
        } else {
            AuditResult::Failure
        },
    ));
    Ok(TareResultDto {
        success: r.success,
        a00_found: r.a00_found,
        weight: r.weight,
    })
}

#[tauri::command]
fn subscribe_telemetry(app: AppHandle, channel: Channel<TelemetrySnapshot>) -> Result<(), String> {
    let st = app.state::<AppState>();
    current_user(&st)?;
    let temp_rx = st.hub.temp_rx();
    let mfc_rx = st.hub.mfc_rx();
    let bal_rx = st.hub.balance_rx();
    // Each subscribe claims a new generation; the pump exits when the generation moves on
    // (a re-subscribe or unsubscribe_telemetry) or the channel drops — so stale pumps from
    // StrictMode double-invokes / HMR never accumulate (issue #8).
    let gen = st.telemetry_gen.clone();
    let my_gen = gen.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let mut seq = 0u64;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if gen.load(Ordering::SeqCst) != my_gen {
                break;
            }
            seq += 1;
            let t = temp_rx.borrow().clone();
            let m = mfc_rx.borrow().clone();
            let b = bal_rx.borrow().clone();
            if channel.send(aggregate(seq, &t, &m, &b)).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// Stop the current telemetry pump thread (issue #8). The frontend calls this on unsubscribe.
#[tauri::command]
fn unsubscribe_telemetry(state: State<AppState>) -> Result<(), String> {
    current_user(&state)?;
    state.telemetry_gen.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

// ---- Experiment -------------------------------------------------------------

#[tauri::command]
fn get_experiment_state(state: State<AppState>) -> Result<ExperimentStatus, String> {
    let user = current_user(&state)?;
    let mut status = state.exp.snapshot();
    let owns = status.owner_user_id.as_deref() == Some(user.user_id.as_str());
    status.can_control = user.role.is_admin() || owns;
    status.occupied_by_other = status.running && !status.can_control;
    if status.occupied_by_other {
        status.sample_name = None;
        status.owner_user_id = None;
        status.owner_display_name = None;
    }
    Ok(status)
}

/// Validate experiment start parameters (docs/05 §5.2). Pure so it is unit-testable.
fn validate_experiment_params(params: &ExperimentParams) -> Result<(), String> {
    if params.sample_id.trim().is_empty() {
        return Err("样品编号必填".into());
    }
    if params.sample_name.trim().is_empty() {
        return Err("样品名称必填".into());
    }
    if params.sample_weight.is_nan() || params.sample_weight <= 0.0 {
        return Err("样品重量必须大于 0".into());
    }
    if !MODES.contains(&params.experiment_type.as_str()) {
        return Err("未知实验模式".into());
    }
    Ok(())
}

#[tauri::command]
fn start_experiment(app: AppHandle, params: ExperimentParams) -> Result<String, String> {
    validate_experiment_params(&params)?;

    let st = app.state::<AppState>();
    let user = current_user(&st)?;
    if st.exp.is_running() {
        return Err("实验已在运行".into());
    }
    let program = standard_program(&params.experiment_type).ok_or("未知实验模式")?;

    let now = iso_now();
    let id = format!("exp-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let row = ExperimentRow {
        experiment_id: id.clone(),
        experiment_name: params.sample_name.clone(),
        sample_name: params.sample_name.clone(),
        sample_weight: params.sample_weight,
        start_time: now.clone(),
        end_time: None,
        description: params.notes.clone(),
        operator: Some(user.display_name.clone()),
        experiment_type: Some(params.experiment_type.clone()),
        analysis_results_json: None,
        created_at: now.clone(),
        owner_user_id: Some(user.user_id.clone()),
    };
    lock(&st.storage)
        .insert_experiment(&row)
        .map_err(|e| e.to_string())?;

    // Reset signals and publish the initial running status before the task spins up.
    st.exp.stop.store(false, Ordering::Relaxed);
    st.exp.skip.store(false, Ordering::Relaxed);
    let first = program.stages.first();
    let initial = ExperimentStatus {
        running: true,
        experiment_id: Some(id.clone()),
        mode: Some(params.experiment_type.clone()),
        sample_name: Some(params.sample_name.clone()),
        started_at: Some(now.clone()),
        phase: Phase::Running,
        stage_index: 0,
        total_stages: program.stages.len(),
        stage_name: first.map(|s| s.name.clone()),
        stage_kind: first.map(|s| s.kind),
        target_temp: first.map(|s| s.target_temp).unwrap_or(0.0),
        tolerance: first.map(|s| s.tolerance).unwrap_or(0.0),
        stage_elapsed_s: 0.0,
        total_elapsed_s: 0.0,
        owner_user_id: Some(user.user_id.clone()),
        owner_display_name: Some(user.display_name.clone()),
        can_control: true,
        occupied_by_other: false,
    };
    st.exp.set_status(initial.clone());
    st.audit.write(
        &AuditEntry::new(Category::Experiment, "start", AuditResult::Success)
            .operator(user.display_name.clone())
            .detail(serde_json::json!({
                "id": id, "mode": params.experiment_type, "project": params.project_name
            })),
    );
    let _ = app.emit("experiment-state-changed", initial);

    spawn_experiment_task(
        app.clone(),
        st.hub.clone(),
        st.storage.clone(),
        st.audit.clone(),
        st.exp.clone(),
        program,
        id.clone(),
        params.experiment_type.clone(),
        params.sample_name.clone(),
        params.sample_weight,
        now,
        user.user_id,
        user.display_name,
        st.sim_speed,
    );
    Ok(id)
}

#[tauri::command]
async fn stop_experiment(app: AppHandle, reason: String) -> Result<(), String> {
    require_experiment_controller(&app.state::<AppState>())?;
    let (hub, audit, exp) = {
        let st = app.state::<AppState>();
        (st.hub.clone(), st.audit.clone(), st.exp.clone())
    };
    // Signal the tick task to finalize (it publishes idle within ≤1s).
    exp.stop.store(true, Ordering::Relaxed);
    // HARD INVARIANT (docs/05 §5.5): apply the safety atmosphere immediately here too —
    // belt-and-suspenders even if the tick task has already died.
    let safety_result = match hub.request_set_atmosphere(SAFETY_ATMOSPHERE).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) | Err(_) => Err("DEVICE_OFFLINE".to_string()),
    };
    hub.set_temp_target_c(25.0);
    audit.write(
        &AuditEntry::new(
            Category::Experiment,
            "stop",
            if safety_result.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .detail(serde_json::json!({
            "reason": reason,
            "safety_atmosphere_confirmed": safety_result.is_ok()
        })),
    );
    // If nothing was running, there is no task to publish idle — do it here.
    if !exp.is_running() {
        let status = if safety_result.is_ok() {
            ExperimentStatus::idle()
        } else {
            ExperimentStatus::safety_error("manual-stop", 0.0)
        };
        exp.set_status(status.clone());
        let _ = app.emit("experiment-state-changed", status);
    }
    safety_result
}

#[tauri::command]
fn skip_stage(state: State<AppState>) -> Result<(), String> {
    require_experiment_controller(&state)?;
    if !state.exp.is_running() {
        return Err("没有正在运行的实验".into());
    }
    state.exp.skip.store(true, Ordering::Relaxed);
    Ok(())
}

/// The three GB standard programs (docs/05 §5.3) for the experiment-mode page.
#[tauri::command]
fn list_programs(state: State<AppState>) -> Result<Vec<Program>, String> {
    current_user(&state)?;
    Ok(tmh_core::engine::STANDARD_MODES
        .iter()
        .filter_map(|m| standard_program(m))
        .collect())
}

// ---- History query ----------------------------------------------------------

#[derive(serde::Serialize)]
struct ExperimentSummary {
    experiment_id: String,
    experiment_name: String,
    sample_name: String,
    sample_weight: f64,
    start_time: String,
    end_time: Option<String>,
    operator: Option<String>,
    experiment_type: Option<String>,
    has_analysis: bool,
    owner_user_id: Option<String>,
}

fn summary_of(r: ExperimentRow) -> ExperimentSummary {
    ExperimentSummary {
        has_analysis: r.analysis_results_json.is_some(),
        experiment_id: r.experiment_id,
        experiment_name: r.experiment_name,
        sample_name: r.sample_name,
        sample_weight: r.sample_weight,
        start_time: r.start_time,
        end_time: r.end_time,
        operator: r.operator,
        experiment_type: r.experiment_type,
        owner_user_id: r.owner_user_id,
    }
}

#[derive(serde::Serialize)]
struct ExperimentDetail {
    experiment_id: String,
    experiment_name: String,
    sample_name: String,
    sample_weight: f64,
    start_time: String,
    end_time: Option<String>,
    description: Option<String>,
    operator: Option<String>,
    experiment_type: Option<String>,
    analysis: Option<serde_json::Value>,
    created_at: String,
    sample_count: i64,
    owner_user_id: Option<String>,
}

fn detail_of(r: ExperimentRow, sample_count: i64) -> ExperimentDetail {
    let analysis = r
        .analysis_results_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());
    ExperimentDetail {
        experiment_id: r.experiment_id,
        experiment_name: r.experiment_name,
        sample_name: r.sample_name,
        sample_weight: r.sample_weight,
        start_time: r.start_time,
        end_time: r.end_time,
        description: r.description,
        operator: r.operator,
        experiment_type: r.experiment_type,
        analysis,
        created_at: r.created_at,
        sample_count,
        owner_user_id: r.owner_user_id,
    }
}

#[derive(serde::Serialize)]
struct SampleDto {
    timestamp: String,
    experiment_duration: Option<String>,
    temperature: f64,
    weight: f64,
    weight_loss: f64,
    co_flow: f64,
    co2_flow: f64,
    n2_flow: f64,
    h2_flow: f64,
    experiment_status: Option<String>,
}

fn sample_dto(r: SampleRow) -> SampleDto {
    SampleDto {
        timestamp: r.timestamp,
        experiment_duration: r.experiment_duration,
        temperature: r.temperature,
        weight: r.weight,
        weight_loss: r.weight_loss,
        co_flow: r.co_flow,
        co2_flow: r.co2_flow,
        n2_flow: r.n2_flow,
        h2_flow: r.h2_flow,
        experiment_status: r.experiment_status,
    }
}

#[tauri::command]
fn list_experiments(state: State<AppState>) -> Result<Vec<ExperimentSummary>, String> {
    let user = current_user(&state)?;
    let storage = lock(&state.storage);
    let rows = if user.role.is_admin() {
        storage.list_experiments()
    } else {
        storage.list_experiments_for_owner(&user.user_id)
    }
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(summary_of).collect())
}

#[tauri::command]
fn get_experiment_detail(state: State<AppState>, id: String) -> Result<ExperimentDetail, String> {
    let user = current_user(&state)?;
    authorize_experiment(&state, &id, &user)?;
    let s = lock(&state.storage);
    let row = s
        .get_experiment(&id)
        .map_err(|e| e.to_string())?
        .ok_or("实验不存在")?;
    let count = s.count_samples(&id).map_err(|e| e.to_string())?;
    Ok(detail_of(row, count))
}

#[tauri::command]
fn get_experiment_samples(
    state: State<AppState>,
    id: String,
    max_points: Option<usize>,
) -> Result<Vec<SampleDto>, String> {
    let user = current_user(&state)?;
    authorize_experiment(&state, &id, &user)?;
    let rows = lock(&state.storage)
        .get_samples(&id, max_points.unwrap_or(2000))
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(sample_dto).collect())
}

#[tauri::command]
fn delete_experiment(state: State<AppState>, id: String) -> Result<(), String> {
    current_admin(&state)?;
    // Never delete the run currently in progress.
    if state.exp.snapshot().experiment_id.as_deref() == Some(id.as_str()) {
        return Err("无法删除正在运行的实验".into());
    }
    lock(&state.storage)
        .delete_experiment(&id)
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(Category::Experiment, "delete", AuditResult::Success)
            .detail(serde_json::json!({ "id": id })),
    );
    Ok(())
}

// ---- GB analysis ------------------------------------------------------------

/// Parse "HH:MM:SS" → minutes (for the reduction-rate least-squares fit).
fn parse_hms_min(s: &str) -> Option<f64> {
    let mut it = s.split(':');
    let h: f64 = it.next()?.parse().ok()?;
    let m: f64 = it.next()?.parse().ok()?;
    let sec: f64 = it.next()?.parse().ok()?;
    Some(h * 60.0 + m + sec / 60.0)
}

/// Persist an analysis-result JSON onto the experiment row and audit it.
fn persist_analysis(
    state: &AppState,
    id: &str,
    standard: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
    lock(&state.storage)
        .set_analysis_results(id, &json)
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(Category::Experiment, "analyze", AuditResult::Success)
            .detail(serde_json::json!({ "id": id, "standard": standard })),
    );
    Ok(())
}

fn require_experiment(state: &AppState, id: &str) -> Result<(), String> {
    let user = current_user(state)?;
    authorize_experiment(state, id, &user).map(|_| ())
}

/// GB/T 13241 reducibility: Rt from W0 (sample weight) and Wt (final sample weight),
/// plus the reduction-rate index over the recorded curve. `o2_pct` is operator-entered.
#[tauri::command]
fn analyze_gb13241(
    state: State<AppState>,
    id: String,
    o2_pct: f64,
) -> Result<serde_json::Value, String> {
    let (w0, samples) = {
        let s = lock(&state.storage);
        let row = s
            .get_experiment(&id)
            .map_err(|e| e.to_string())?
            .ok_or("实验不存在")?;
        let samples = s.get_samples(&id, 0).map_err(|e| e.to_string())?;
        (row.sample_weight, samples)
    };
    let wt = samples.last().map(|r| r.weight).unwrap_or(w0);
    let red = reducibility(w0, wt, o2_pct).ok_or("W0 或 O2% 无效")?;
    let points: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|r| {
            let m = parse_hms_min(r.experiment_duration.as_deref()?)?;
            Some((m, reducibility(w0, r.weight, o2_pct)?.rt))
        })
        .collect();
    let value = serde_json::json!({
        "standard": "GB/T 13241",
        "w0": w0,
        "wt": wt,
        "o2_pct": o2_pct,
        "rt": red.rt,
        "delta_w": red.delta_w,
        "reduction_rate_index": reduction_rate_index(&points),
        "computed_at": iso_now(),
    });
    persist_analysis(&state, &id, "GB/T 13241", &value)?;
    Ok(value)
}

/// GB/T 13242 low-temperature degradation RDI from operator-entered sieve masses (g).
#[tauri::command]
fn analyze_gb13242(
    state: State<AppState>,
    id: String,
    m0: f64,
    m_gt63: f64,
    m_315_63: f64,
    m_05_315: f64,
    m_lt05: f64,
) -> Result<serde_json::Value, String> {
    require_experiment(&state, &id)?;
    let r = rdi(m0, m_gt63, m_315_63, m_05_315, m_lt05).ok_or("m0 无效")?;
    let value = serde_json::json!({
        "standard": "GB/T 13242",
        "m0": m0,
        "m_gt63": m_gt63,
        "m_315_63": m_315_63,
        "m_05_315": m_05_315,
        "m_lt05": m_lt05,
        "rdi_plus_6_3": r.rdi_plus_6_3,
        "rdi_plus_3_15": r.rdi_plus_3_15,
        "rdi_minus_0_5": r.rdi_minus_0_5,
        "sieve_warning": r.sieve_warning,
        "computed_at": iso_now(),
    });
    persist_analysis(&state, &id, "GB/T 13242", &value)?;
    Ok(value)
}

/// GB/T 13240 free-swelling index from operator-entered (V0, V1) volume pairs (mL).
#[tauri::command]
fn analyze_gb13240(
    state: State<AppState>,
    id: String,
    volumes: Vec<(f64, f64)>,
) -> Result<serde_json::Value, String> {
    require_experiment(&state, &id)?;
    let r = free_swelling(&volumes).ok_or("无有效试样体积")?;
    let value = serde_json::json!({
        "standard": "GB/T 13240",
        "per_pellet": r.per_pellet,
        "average": r.average,
        "computed_at": iso_now(),
    });
    persist_analysis(&state, &id, "GB/T 13240", &value)?;
    Ok(value)
}

// ---- Report & export --------------------------------------------------------

/// The reducing-stage target temperature for a mode (for report condition checks).
fn reducing_target(mode: Option<&str>) -> Option<f64> {
    mode.and_then(standard_program).and_then(|p| {
        p.stages
            .into_iter()
            .find(|s| s.kind == StageKind::Reducing)
            .map(|s| s.target_temp)
    })
}

/// Generate a self-contained HTML report to the exports dir and open it. Returns the path.
#[tauri::command]
fn generate_report(app: AppHandle, id: String) -> Result<String, String> {
    let st = app.state::<AppState>();
    let user = current_user(&st)?;
    authorize_experiment(&st, &id, &user)?;
    let (exp, samples) = {
        let s = lock(&st.storage);
        let exp = s
            .get_experiment(&id)
            .map_err(|e| e.to_string())?
            .ok_or("实验不存在")?;
        let samples = s.get_samples(&id, 0).map_err(|e| e.to_string())?;
        (exp, samples)
    };
    let analysis = exp
        .analysis_results_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok());
    let html = tmh_core::report::render_report_html(
        &exp,
        &samples,
        analysis.as_ref(),
        reducing_target(exp.experiment_type.as_deref()),
    );
    let user_dir = st.exports_dir.join(&user.user_id);
    std::fs::create_dir_all(&user_dir).map_err(|e| e.to_string())?;
    let path = user_dir.join(format!("report-{id}.html"));
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    let p = path.to_string_lossy().to_string();
    let _ = app.opener().open_path(p.clone(), None::<&str>);
    st.audit.write(
        &AuditEntry::new(
            Category::Experiment,
            "generate_report",
            AuditResult::Success,
        )
        .detail(serde_json::json!({ "id": id, "path": p })),
    );
    Ok(p)
}

/// Export the sample series as csv / txt / xlsx to the exports dir and open it.
#[tauri::command]
fn export_experiment(app: AppHandle, id: String, format: String) -> Result<String, String> {
    let st = app.state::<AppState>();
    let user = current_user(&st)?;
    authorize_experiment(&st, &id, &user)?;
    let (exp, samples) = {
        let s = lock(&st.storage);
        let exp = s
            .get_experiment(&id)
            .map_err(|e| e.to_string())?
            .ok_or("实验不存在")?;
        let samples = s.get_samples(&id, 0).map_err(|e| e.to_string())?;
        (exp, samples)
    };
    let (bytes, ext) = match format.as_str() {
        "csv" => (
            tmh_core::report::samples_to_csv(&samples).into_bytes(),
            "csv",
        ),
        "txt" => (
            tmh_core::report::samples_to_tsv(&samples).into_bytes(),
            "txt",
        ),
        "xlsx" => (tmh_core::report::samples_to_xlsx(&exp, &samples)?, "xlsx"),
        _ => return Err("未知导出格式".into()),
    };
    let user_dir = st.exports_dir.join(&user.user_id);
    std::fs::create_dir_all(&user_dir).map_err(|e| e.to_string())?;
    let path = user_dir.join(format!("{id}.{ext}"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let p = path.to_string_lossy().to_string();
    let _ = app.opener().open_path(p.clone(), None::<&str>);
    st.audit.write(
        &AuditEntry::new(Category::Experiment, "export", AuditResult::Success)
            .detail(serde_json::json!({ "id": id, "format": format, "path": p })),
    );
    Ok(p)
}

// ---- Software update (docs/09 §9.3) -----------------------------------------

#[derive(serde::Serialize, Clone)]
struct UpdateCheck {
    /// Whether update checking is enabled (config/env gate; OFF by default).
    enabled: bool,
    current_version: String,
    available: bool,
    new_version: Option<String>,
    notes: Option<String>,
    /// A friendly message when the manifest endpoint is unreachable (offline工控机).
    error: Option<String>,
}

impl UpdateCheck {
    fn base(enabled: bool, current: String) -> Self {
        Self {
            enabled,
            current_version: current,
            available: false,
            new_version: None,
            notes: None,
            error: None,
        }
    }
}

#[tauri::command]
fn get_updater_status(app: AppHandle) -> UpdateCheck {
    let enabled = app.state::<AppState>().updater_enabled;
    UpdateCheck::base(enabled, app.package_info().version.to_string())
}

/// Check the manifest endpoint for a newer version. Gated; never errors into the UI —
/// an unreachable endpoint yields a friendly `error` string (offline is normal here).
#[tauri::command]
async fn check_for_update(app: AppHandle) -> UpdateCheck {
    let current = app.package_info().version.to_string();
    let (enabled, audit) = {
        let st = app.state::<AppState>();
        (st.updater_enabled, st.audit.clone())
    };
    let mut check = UpdateCheck::base(enabled, current);
    if !enabled {
        return check;
    }
    match app.updater() {
        Ok(updater) => match updater.check().await {
            Ok(Some(update)) => {
                check.available = true;
                check.new_version = Some(update.version.clone());
                check.notes = update.body.clone();
            }
            Ok(None) => {}
            Err(e) => {
                check.error = Some(format!("无法连接更新服务器（离线环境属正常）：{e}"));
            }
        },
        Err(e) => check.error = Some(format!("更新器不可用：{e}")),
    }
    audit.write(
        &AuditEntry::new(
            Category::App,
            "check_update",
            if check.error.is_some() {
                AuditResult::Failure
            } else {
                AuditResult::Success
            },
        )
        .detail(
            serde_json::json!({ "available": check.available, "new_version": check.new_version }),
        ),
    );
    let _ = app.emit("update-status", &check);
    check
}

/// Download + verify (minisign) + install the update, then restart. Gated.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    if !app.state::<AppState>().updater_enabled {
        return Err("更新功能未启用".into());
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("已是最新版本")?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.state::<AppState>().audit.write(&AuditEntry::new(
        Category::App,
        "install_update",
        AuditResult::Success,
    ));
    app.restart();
}

// ---- App entrypoint ---------------------------------------------------------

/// Build and run the Tauri application.
pub fn run() {
    let mode = std::env::var("TMH_DEVICE_MODE").unwrap_or_else(|_| "mock".into());
    let sim_speed = sim_speed(&mode);
    // Software update checking is OFF by default (docs/09 §9.3); config/env enables it.
    let updater_enabled = matches!(
        std::env::var("TMH_UPDATER").as_deref(),
        Ok("1" | "on" | "true")
    );
    // Low-end 工控核显 escape hatch (docs/09 §9.1): disable GPU accel on demand.
    if std::env::var("TMH_DISABLE_GPU").is_ok() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-gpu");
    }
    // Persistent per-user data dir (Windows %APPDATA%, Linux ~/.local/share, etc.);
    // falls back to a temp dir only if the platform dir is unavailable. Experiment
    // history and the 180-day audit trail must survive OS temp cleanup (issue #6).
    let data_root = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("tmh-lpf-900");
    let audit_dir = data_root.join("logs");
    let db_dir = data_root.join("data");
    let exports_dir = data_root.join("exports");
    let _ = std::fs::create_dir_all(&audit_dir);
    let _ = std::fs::create_dir_all(&db_dir);
    let _ = std::fs::create_dir_all(&exports_dir);
    let storage = Storage::open(&db_dir.join("experiments.db")).expect("open experiments db");
    let comm_config = load_comm_config();
    let hub = Arc::new(build_mock_hub(&comm_config));
    let audit = Arc::new(AuditLog::new(audit_dir));
    // Enforce the documented 180-day audit retention (docs/06 §6.5) at startup — issue #6.
    audit.prune(180);

    // Crash recovery before the UI comes up (safety atmosphere for any open run).
    recover_unfinished(&hub, &storage, &audit);

    let state = AppState {
        hub,
        mode,
        limit_h2: comm_config.gas_safety_limits.h2,
        limit_co: comm_config.gas_safety_limits.co,
        audit,
        storage: Arc::new(Mutex::new(storage)),
        auth: Arc::new(AuthService::new(15 * 60)),
        exp: Arc::new(ExpControl::new()),
        sim_speed,
        exports_dir,
        updater_enabled,
        telemetry_gen: Arc::new(AtomicU64::new(0)),
    };

    tauri::Builder::default()
        // Single-instance MUST be registered first (serial-port exclusivity, docs/03).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            app.state::<AppState>().audit.write(&AuditEntry::new(
                Category::App,
                "start_blocked",
                AuditResult::Rejected,
            ));
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_auth_state,
            bootstrap_admin,
            login,
            logout,
            lock_session,
            touch_session,
            change_own_password,
            list_users,
            create_user,
            update_user,
            reset_user_password,
            set_experiment_owner,
            get_public_safety_status,
            emergency_safe_shutdown,
            get_app_info,
            get_runtime_mode,
            get_gas_safety_limits,
            get_device_health,
            set_gas_flow,
            set_gas_on_off,
            open_all_valves,
            close_all_valves,
            shutoff_flammable_gases,
            tare_balance,
            subscribe_telemetry,
            unsubscribe_telemetry,
            get_experiment_state,
            start_experiment,
            stop_experiment,
            skip_stage,
            list_programs,
            list_experiments,
            get_experiment_detail,
            get_experiment_samples,
            delete_experiment,
            analyze_gb13241,
            analyze_gb13242,
            analyze_gb13240,
            generate_report,
            export_experiment,
            get_updater_status,
            check_for_update,
            install_update
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Every application-exit path must leave the furnace in the protective
            // atmosphere (docs/05 §5.5, issue #1). `ExitRequested` fires before teardown;
            // apply the safety atmosphere synchronously here so gas is never left flowing
            // after the window closes. The MFC bus thread also zeroes flammables on stop.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let st = app.state::<AppState>();
                let result = apply_safety_atmosphere_blocking(&st.hub);
                if result.is_err() {
                    // A normal window-close must not tear the process down after the
                    // protective atmosphere failed. Keep the actor alive for retry/operator
                    // intervention; OS-forced termination still falls back to Drop's best effort.
                    api.prevent_exit();
                }
                st.audit.write(&AuditEntry::new(
                    Category::App,
                    "exit",
                    if result.is_ok() {
                        AuditResult::Success
                    } else {
                        AuditResult::Failure
                    },
                ));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(edit: impl FnOnce(&mut ExperimentParams)) -> ExperimentParams {
        let mut p = ExperimentParams {
            sample_id: "S1".into(),
            sample_name: "样品".into(),
            sample_weight: 100.0,
            experiment_type: "REDUCIBILITY".into(),
            project_name: None,
            notes: None,
        };
        edit(&mut p);
        p
    }

    #[test]
    fn valid_experiment_params_pass() {
        assert!(validate_experiment_params(&params(|_| {})).is_ok());
    }

    #[test]
    fn blank_or_bad_experiment_params_are_rejected() {
        assert!(validate_experiment_params(&params(|p| p.sample_id = "  ".into())).is_err());
        assert!(validate_experiment_params(&params(|p| p.sample_name = "".into())).is_err());
        assert!(validate_experiment_params(&params(|p| p.sample_weight = 0.0)).is_err());
        assert!(validate_experiment_params(&params(|p| p.sample_weight = f64::NAN)).is_err());
        assert!(
            validate_experiment_params(&params(|p| p.experiment_type = "NOPE".into())).is_err()
        );
    }

    #[test]
    fn hms_minutes_parses() {
        assert_eq!(parse_hms_min("01:30:00"), Some(90.0));
        assert_eq!(parse_hms_min("00:00:30"), Some(0.5));
        assert_eq!(parse_hms_min("bad"), None);
    }

    #[test]
    fn reducing_target_matches_program() {
        assert_eq!(reducing_target(Some("REDUCIBILITY")), Some(900.0));
        assert_eq!(reducing_target(Some("LOW_TEMP_DEGRADATION")), Some(500.0));
        assert_eq!(reducing_target(Some("bogus")), None);
    }

    #[test]
    fn bundled_default_comm_config_is_valid_and_matches_docs_06() {
        // Guards against config/comm_config.json drifting from docs/06 §6.4 again (#11).
        let c = CommConfig::from_json(DEFAULT_COMM_CONFIG).expect("parse");
        c.validate().expect("valid");
        assert_eq!(c.gas_safety_limits.h2, 5.0);
        assert_eq!(c.slave_address_mfc.co, 4);
        assert_eq!(c.balance.baudrate, 1200);
        assert_eq!(c.mfc.parity, "E");
        assert_eq!(c.temp.slave_address, 0);
    }

    #[test]
    fn load_comm_config_returns_a_valid_config() {
        // The test working dir has no config/comm_config.json, so this exercises the
        // compiled-in default fallback path (#11).
        load_comm_config()
            .validate()
            .expect("fallback config valid");
    }
}
