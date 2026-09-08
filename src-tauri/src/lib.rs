//! Thin Tauri glue (docs/03). Owns the M2 `DeviceHub` in managed state, exposes it
//! to the webview through typed commands + a telemetry Channel, and hardens the shell
//! (single-instance). M4 runs the real experiment engine: a per-run tick task drives
//! the furnace through its program stages, applies each stage's atmosphere through the
//! safety clamp and samples to SQLite. Normal endings follow the selected procedure;
//! stops and faults apply the protective atmosphere.

use tmh_core::recovery;
use tmh_core::storage::RunCompletion;

mod auth_service;
mod auth_session;
mod custom_programs;
mod persistence;
mod procedure;
use persistence::recover_unfinished;
#[cfg(test)]
use persistence::retry_pending_completions;
use tmh_core::provenance::validate_executable_program;

use auth_service::{AuthCode, AuthService, AuthSnapshot, AuthStatus, PublicUser};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use tmh_core::audit::{AuditEntry, AuditLog, AuditResult, Category};
use tmh_core::auth::{hash_password, validate_display_name, warm_password_hashing, UserRole};
use tmh_core::bus::{BalanceActor, DeviceHub, HubIntervals, MfcActor, TempActor};
use tmh_core::clamp::ClampResult;
use tmh_core::config::CommConfig;
use tmh_core::engine::{
    standard_program, ExperimentEngine, Phase, Program, ProgramSource, Stage, StageKind,
    STANDARD_MODES,
};
use tmh_core::gb::{
    free_swelling, rdi, reducibility, reduction_rate, RdiInput, ReductionInput, SwellingInput,
    CALCULATION_VERSION,
};
use tmh_core::sampler::{build_sample_row, SampleContext};
use tmh_core::storage::{ExperimentRow, SampleRow, Storage, UserRow};
use tmh_core::telemetry::{aggregate, TelemetrySnapshot};
use tmh_core::transport::MockTransport;

// IPC argument names: Tauri derives each command's argument key from the Rust parameter
// name and converts it to lowerCamelCase unless the command opts out with
// `rename_all = "snake_case"`. Every command below that takes a multi-word parameter
// carries that attribute so the key stays snake_case on both sides of the boundary, and
// `scripts/check-ipc-contract.mjs` fails the build if the two ever drift apart (#40).

/// Number of gas channels (order: H2, N2, CO2, CO).
const GAS_COUNT: usize = 4;
/// Mock reduction removes this fraction of the sample mass (so GB analysis has signal).
const MOCK_REDUCTION_LOSS: f64 = 0.12;
/// Flush buffered samples to SQLite every N samples.
const FLUSH_EVERY_SAMPLES: u32 = 5;
/// Upper bound on one control-loop tick, independent of the configured sampling interval.
/// The loop that evaluates the stop flag, the bus-offline abort and stage transitions must
/// stay responsive: a site that configures a slow `SAMPLING.interval_s` may space out its
/// data rows, but must never delay cutting the gas when a safety-critical bus drops.
const CONTROL_TICK_MAX: Duration = Duration::from_secs(1);
/// Cap on buffered-but-unwritten sample rows (~33 min at 1 Hz). If SQLite keeps rejecting
/// writes, the buffer must not grow without bound for the length of a multi-hour run (#47).
const MAX_BUFFERED_SAMPLES: usize = 2000;
/// Re-audit a persistent flush failure every N attempts (the first failure always audits).
const FLUSH_FAILURE_AUDIT_EVERY: u32 = 12;

/// Lock a mutex tolerating poisoning: a panic while the lock was held must not turn every
/// later command into a panic (our guarded data stays consistent across such panics) (#14).
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Managed application state.
struct AppState {
    _data_lock: recovery::DataDirectoryLock,
    recovery_dir: PathBuf,
    hub: Arc<DeviceHub>,
    mode: String,
    limit_h2: f64,
    limit_co: f64,
    flow_steps: [f64; 4],
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
    /// Cadences from `comm_config.json` (docs/06 §6.3). These used to be hardcoded, so
    /// edits to the field config were silently ignored (#44).
    cadence: Cadence,
    /// Idle lock timeout for the session, in seconds (#46).
    idle_timeout_s: i64,
}

/// Configured cadences (docs/06 §6.3 / §6.4).
#[derive(Clone, Copy)]
struct Cadence {
    /// `SAMPLING.interval_s` — one experiment sample row per period.
    sampling: Duration,
    /// `PERFORMANCE_CONFIG.data_collection_interval` — telemetry aggregation to the UI.
    telemetry: Duration,
    /// `PERFORMANCE_CONFIG.heartbeat_timeout` — the UI greys a snapshot older than this.
    heartbeat_timeout_s: f64,
}

impl Cadence {
    fn from_config(cfg: &CommConfig) -> Self {
        Self {
            sampling: seconds(cfg.sampling.interval_s),
            telemetry: seconds(cfg.performance.data_collection_interval),
            heartbeat_timeout_s: cfg.performance.heartbeat_timeout,
        }
    }
}

/// Config seconds → `Duration`, floored at 10 ms so a bad value can never spin a loop.
/// `CommConfig::validate` already rejects non-positive cadences; this is the second line.
fn seconds(value: f64) -> Duration {
    Duration::from_secs_f64(value.max(0.01))
}

/// Shared control block for the single running experiment. `status` is the published
/// snapshot the UI polls/receives; `stop`/`skip` are one-shot signals the tick task
/// observes on its next cycle.
struct ExpControl {
    lifecycle: Mutex<()>,
    updating: AtomicBool,
    pending: Mutex<Vec<RunCompletion>>,
    recovery_errors: Mutex<Vec<String>>,
    status: Mutex<ExperimentStatus>,
    stop: AtomicBool,
    stage_action: Mutex<Option<procedure::StageAction>>,
}

impl ExpControl {
    fn new() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            updating: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
            recovery_errors: Mutex::new(Vec::new()),
            status: Mutex::new(ExperimentStatus::idle()),
            stop: AtomicBool::new(false),
            stage_action: Mutex::new(None),
        }
    }
    fn snapshot(&self) -> ExperimentStatus {
        let mut status = lock(&self.status).clone();
        status.persistence_pending = self.persistence_pending();
        status.updating = self.updating.load(Ordering::SeqCst);
        status
    }
    fn persistence_pending(&self) -> bool {
        !lock(&self.pending).is_empty() || !lock(&self.recovery_errors).is_empty()
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
    updating: bool,
    persistence_pending: bool,
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
    stable_elapsed_s: f64,
    total_elapsed_s: f64,
    owner_user_id: Option<String>,
    owner_display_name: Option<String>,
    can_control: bool,
    occupied_by_other: bool,
}

impl ExperimentStatus {
    fn idle() -> Self {
        Self {
            updating: false,
            persistence_pending: false,
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
            stable_elapsed_s: 0.0,
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

/// Display name of the session user, for audit attribution. Reads the session WITHOUT
/// touching its idle deadline, so auditing an action never keeps a session alive, and
/// falls back to "anonymous" for the unauthenticated safety commands. Every audited action
/// must name who performed it — on a shared instrument the audit trail is the only record
/// of who opened the flammable gases (docs/06 §6.5, #41).
fn audit_operator(state: &AppState) -> String {
    state
        .auth
        .snapshot(&lock(&state.storage), epoch_now())
        .ok()
        .and_then(|snapshot| snapshot.user)
        .map(|user| user.display_name)
        .unwrap_or_else(|| "anonymous".to_string())
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
    if may_access_experiment(user.role, &user.user_id, row.owner_user_id.as_deref()) {
        Ok(row)
    } else {
        Err(AuthCode::Forbidden.to_string())
    }
}

fn may_access_experiment(role: UserRole, user_id: &str, owner_user_id: Option<&str>) -> bool {
    role.is_admin() || owner_user_id == Some(user_id)
}

fn require_experiment_controller(state: &AppState) -> Result<UserRow, String> {
    if state.exp.updating.load(Ordering::SeqCst) {
        return Err("正在安装更新，暂不能修改设备或实验".into());
    }
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

fn redact_experiment_status(
    mut status: ExperimentStatus,
    role: UserRole,
    user_id: &str,
) -> ExperimentStatus {
    let owns = status.owner_user_id.as_deref() == Some(user_id);
    status.can_control = role.is_admin() || owns;
    status.occupied_by_other = status.running && !status.can_control;
    if status.occupied_by_other {
        status.sample_name = None;
        status.owner_user_id = None;
        status.owner_display_name = None;
    }
    status
}

/// Emit only a view authorized for the session that is active at emission time. Snapshotting
/// does not touch the idle deadline, so a running experiment cannot keep a user logged in.
fn emit_experiment_status(app: &AppHandle, status: &ExperimentStatus) {
    let state = app.state::<AppState>();
    let snapshot = state.auth.snapshot(&lock(&state.storage), epoch_now()).ok();
    if let Some(snapshot) = snapshot {
        if matches!(snapshot.status, AuthStatus::Authenticated) {
            if let Some(user) = snapshot.user {
                let visible = redact_experiment_status(status.clone(), user.role, &user.user_id);
                let _ = app.emit("experiment-state-changed", visible);
            }
        }
    }
}

/// Phase → stable string for the `experiment_status` sample column.
fn custom_program_id() -> String {
    format!("custom-{}", tmh_core::auth::new_user_id())
}

fn clean_opt(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn sample_prefix_fallback(id: &str) -> &'static str {
    match id {
        "REDUCIBILITY" => "RED",
        "LOW_TEMP_DEGRADATION" => "RDI",
        "FREE_SWELLING" => "FSI",
        _ => "CUS",
    }
}

fn normalize_sample_prefix(raw: &str, id: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_ascii_uppercase();
    if cleaned.len() < 2 {
        sample_prefix_fallback(id).into()
    } else {
        cleaned
    }
}

fn normalize_custom_program(program: &mut Program) {
    program.source = ProgramSource::Custom;
    program.id = program.id.trim().to_string();
    program.name = program.name.trim().to_string();
    program.description = program.description.trim().to_string();
    if program.category.trim().is_empty() || program.category == "standard" {
        program.category = "custom".into();
    } else {
        program.category = program.category.trim().to_string();
    }
    program.sample_prefix = normalize_sample_prefix(&program.sample_prefix, &program.id);
    for stage in &mut program.stages {
        stage.name = stage.name.trim().to_string();
        stage.description = stage.description.trim().to_string();
    }
}

fn read_custom_programs(storage: &Storage) -> Result<Vec<Program>, String> {
    storage
        .list_custom_programs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|json| {
            let p: Program = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(p)
        })
        .collect()
}

fn standard_programs() -> Vec<Program> {
    STANDARD_MODES
        .iter()
        .filter_map(|m| standard_program(m))
        .collect()
}

fn all_programs(storage: &Storage) -> Result<Vec<Program>, String> {
    let mut programs = standard_programs();
    programs.extend(read_custom_programs(storage)?);
    Ok(programs)
}

fn program_by_id(state: &AppState, id: &str) -> Result<Program, String> {
    if let Some(p) = standard_program(id) {
        return Ok(p);
    }
    read_custom_programs(&lock(&state.storage))?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "未知实验模式".to_string())
}

/// Phase → stable string for the `experiment_status` sample column.
fn kind_str(k: StageKind) -> &'static str {
    match k {
        StageKind::Heating => "HEATING",
        StageKind::Stabilizing => "STABILIZING",
        StageKind::Reducing => "REDUCING",
        StageKind::Cooling => "COOLING",
        StageKind::Purging => "PURGING",
        StageKind::Manual => "MANUAL",
    }
}

/// Default session idle-lock timeout (docs/superpowers spec: 15 minutes).
const DEFAULT_IDLE_TIMEOUT_S: i64 = 15 * 60;

/// Session idle-lock timeout: `TMH_IDLE_TIMEOUT_S` if set, else the 15-minute default.
/// Clamped to [1 min, 8 h] so neither a typo nor a hostile value disables the lock or
/// makes the app unusable (#46).
fn idle_timeout_s() -> i64 {
    std::env::var("TMH_IDLE_TIMEOUT_S")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.clamp(60, 8 * 60 * 60))
        .unwrap_or(DEFAULT_IDLE_TIMEOUT_S)
}

/// Simulated-time acceleration: `TMH_SIM_SPEED` if set, else fast for mock, real (1.0)
/// for hardware. Mock runs at 180× so a multi-hour program plays out in a few minutes.
fn validate_runtime_mode(mode: &str) -> Result<(), String> {
    if mode == "mock" {
        Ok(())
    } else {
        Err(format!(
            "当前版本仅支持 mock；{mode:?} 尚未接入，拒绝以模拟设备冒充该模式"
        ))
    }
}

fn sim_speed(mode: &str) -> f64 {
    std::env::var("TMH_SIM_SPEED")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
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
    if stage.kind != StageKind::Purging {
        hub.set_temp_target_c(
            if matches!(stage.kind, StageKind::Cooling | StageKind::Manual) {
                25.0
            } else {
                stage.target_temp
            },
        );
    }
    if stage.kind == StageKind::Reducing {
        // Mock only: reduction strips oxygen, so the sample loses mass over the stage.
        hub.set_balance_target_g(w0 * (1.0 - MOCK_REDUCTION_LOSS));
    }
    Ok(())
}

/// Protective atmosphere for stop/fault/exit: N2=5.0, H2=CO2=CO=0.
fn apply_safety_atmosphere(hub: &DeviceHub) -> Result<(), String> {
    let result = hub
        .request_shutoff_flammable()
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

fn timed_sample(
    hub: &DeviceHub,
    id: &str,
    w0: f64,
    total_s: f64,
    stage_s: f64,
    status: &str,
) -> SampleRow {
    let mut row = build_sample(hub, id, w0, total_s, status);
    let mut record: serde_json::Value =
        serde_json::from_str(row.record_json.as_deref().unwrap_or("{}")).unwrap_or_default();
    record["elapsed_s"] = serde_json::json!(total_s);
    record["stage_elapsed_s"] = serde_json::json!(stage_s);
    row.record_json = Some(record.to_string());
    row
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
        updating: false,
        persistence_pending: false,
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
        stable_elapsed_s: engine.stable_elapsed_s(),
        total_elapsed_s: engine.total_elapsed_s(),
        owner_user_id: Some(owner_user_id.to_string()),
        owner_display_name: Some(owner_display_name.to_string()),
        can_control: true,
        occupied_by_other: false,
    }
}

/// Buffered sample rows plus the bookkeeping needed to report persistent write failures.
#[derive(Default)]
struct SampleBuffer {
    rows: Vec<SampleRow>,
    consecutive_failures: u32,
    dropped: u64,
}

impl SampleBuffer {
    /// Append a row, discarding the oldest rows once the cap is reached so a run that
    /// cannot write to SQLite degrades to "loses the oldest samples" instead of growing
    /// its buffer for hours (#47).
    fn push(&mut self, row: SampleRow) {
        if self.rows.len() >= MAX_BUFFERED_SAMPLES {
            let excess = self.rows.len() + 1 - MAX_BUFFERED_SAMPLES;
            self.rows.drain(..excess);
            self.dropped += excess as u64;
        }
        self.rows.push(row);
    }
}

/// Flush buffered samples in one transaction. On error the rows are retained for retry and
/// the failure is audited — losing a whole run's samples silently is far worse than a noisy
/// log. Sampling must never take down the control loop, so nothing here propagates.
///
/// Uses the poison-tolerant [`lock`] helper: a panic elsewhere while the storage lock was
/// held used to stop persistence for the rest of the run without any signal (#47).
fn flush(
    storage: &Arc<Mutex<Storage>>,
    audit: &AuditLog,
    buffer: &mut SampleBuffer,
    id: &str,
    operator: &str,
    completing: bool,
) {
    // The terminal batch belongs to commit_completion, including when this tick
    // happens to reach the normal periodic flush boundary.
    if completing || buffer.rows.is_empty() {
        return;
    }
    let outcome = lock(storage).insert_samples(&buffer.rows);
    match outcome {
        Ok(()) => {
            if buffer.consecutive_failures > 0 {
                audit.write(
                    &AuditEntry::new(
                        Category::Experiment,
                        "sample_flush_recovered",
                        AuditResult::Success,
                    )
                    .operator(operator.to_string())
                    .detail(serde_json::json!({
                        "id": id, "rows": buffer.rows.len(), "dropped_samples": buffer.dropped
                    })),
                );
            }
            buffer.rows.clear();
            buffer.consecutive_failures = 0;
        }
        Err(error) => {
            buffer.consecutive_failures = buffer.consecutive_failures.saturating_add(1);
            if buffer.consecutive_failures == 1
                || buffer.consecutive_failures % FLUSH_FAILURE_AUDIT_EVERY == 0
            {
                audit.write(
                    &AuditEntry::new(
                        Category::Experiment,
                        "sample_flush_failed",
                        AuditResult::Failure,
                    )
                    .operator(operator.to_string())
                    .detail(serde_json::json!({
                        "id": id,
                        "buffered": buffer.rows.len(),
                        "dropped_samples": buffer.dropped,
                        "attempts": buffer.consecutive_failures,
                        "error": error.to_string(),
                    })),
                );
            }
        }
    }
}

/// Advance a fixed-period schedule. Normally the next wake-up is exactly one period after
/// the previous one, which keeps the cadence free of drift; if the thread was starved for
/// longer than a whole period (suspend/resume, a very slow write) we resynchronise rather
/// than firing a burst of back-to-back ticks to catch up (#43).
/// Control-loop period for a configured sampling interval: never slower than
/// [`CONTROL_TICK_MAX`], so the stop flag, the bus-offline abort and stage advancement stay
/// responsive no matter what cadence `SAMPLING.interval_s` asks for. A faster configured
/// cadence is honoured as-is and samples every tick.
fn control_period(sample_period: Duration) -> Duration {
    sample_period.min(CONTROL_TICK_MAX)
}

fn next_deadline(previous: Instant, period: Duration, now: Instant) -> Instant {
    let next = previous + period;
    if next > now {
        next
    } else {
        now + period
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
    emit_experiment_status(app, &status);
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
    buffer: &mut SampleBuffer,
    id: &str,
    operator: &str,
    outcome: &str,
    total_s: f64,
    normal_gas: Option<[f64; 4]>,
) {
    let normal =
        outcome == "completed" && !exp.stop.load(Ordering::SeqCst) && !hub.safety_latched();
    let outcome = if outcome == "completed" && !normal {
        "stopped"
    } else {
        outcome
    };
    let normal_result = if normal {
        normal_gas
            .ok_or_else(|| "结束气氛缺失".into())
            .and_then(|gas| procedure::confirm_normal_ending(hub, gas))
    } else {
        Ok(())
    };
    if let Err(error) = &normal_result {
        let _ = app.emit(
            "app-notice",
            format!("正常结束未确认，已请求保护气氛：{error}"),
        );
    }
    let outcome = if normal_result.is_err() {
        "error"
    } else {
        outcome
    };
    let safety_result = if normal && normal_result.is_ok() {
        Ok(())
    } else {
        apply_safety_atmosphere(hub)
    };
    let end = iso_now();
    let completion = RunCompletion {
        token: tmh_core::auth::new_user_id(),
        experiment_id: id.into(),
        end_time: end,
        rows: std::mem::take(&mut buffer.rows),
        metadata: serde_json::json!({"outcome":outcome,"data_complete":buffer.dropped==0,"safety_confirmed":safety_result.is_ok(),"dropped_samples":buffer.dropped,"ending_action":if normal && normal_result.is_ok() {"normal"} else {"protective"},"normal_ending_error":normal_result.err()}),
    };
    let recovery_dir = app.state::<AppState>().recovery_dir.clone();
    let journal = recovery::persist(&recovery_dir, &completion);
    let committed = lock(storage).commit_completion(&completion);
    let persisted = committed.is_ok();
    if persisted {
        // A leftover receipt is harmless: startup replay is idempotent.
        if let Err(error) = recovery::clear(&recovery_dir, &completion.token) {
            lock(&exp.recovery_errors).push(error);
        }
    } else {
        lock(&exp.pending).push(completion.clone());
    }
    if normal && committed.is_err() {
        let protection = apply_safety_atmosphere(hub);
        audit.write(
            &AuditEntry::new(
                Category::Experiment,
                "persistence_fault_protection",
                if protection.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
            )
            .operator(operator)
            .detail(serde_json::json!({"id":id,"error":protection.err()})),
        );
    }
    if committed.is_err() || journal.is_err() {
        audit.write(&AuditEntry::new(Category::Experiment,"completion_persistence",if persisted {AuditResult::Success}else{AuditResult::Failure}).operator(operator).detail(serde_json::json!({"id":id,"database_error":committed.err().map(|e|e.to_string()),"journal_error":journal.err(),"retained_rows":if persisted {0}else{completion.rows.len()}})));
    }
    let safety_ok = safety_result.is_ok();
    audit.write(
        &AuditEntry::new(
            Category::Experiment,
            if normal && outcome == "completed" {
                "normal_ending"
            } else {
                "safety_atmosphere"
            },
            if safety_ok {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(operator.to_string())
        .detail(serde_json::json!({ "id": id, "error": safety_result.err() })),
    );
    audit.write(
        &AuditEntry::new(
            Category::Experiment,
            outcome,
            if safety_ok && persisted {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(operator.to_string())
        .detail(serde_json::json!({
            "id": id,
            "elapsed_s": total_s,
            "safety_atmosphere_confirmed": safety_ok,
            "unwritten_samples": if persisted {0} else {completion.rows.len()},
            "persistence_confirmed": persisted,
            "dropped_samples": buffer.dropped
        })),
    );
    let mut final_status = if safety_ok && persisted {
        ExperimentStatus::idle()
    } else {
        ExperimentStatus::safety_error(id, total_s)
    };
    final_status.persistence_pending = exp.persistence_pending();
    exp.set_status(final_status.clone());
    emit_experiment_status(app, &final_status);
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
    sample_period: Duration,
) {
    std::thread::spawn(move || {
        let ending_gas = program.stages.last().map(|s| s.gas);
        let mut engine = ExperimentEngine::start(program);
        let mut buffer = SampleBuffer::default();
        let initial_switch = Instant::now();
        if let Some(s) = engine.current_stage() {
            if apply_stage_atmosphere(&hub, s, w0).is_err() {
                audit.write(
                    &AuditEntry::new(
                        Category::Experiment,
                        "stage_atmosphere",
                        AuditResult::Failure,
                    )
                    .operator(owner_display_name.clone())
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
                    &owner_display_name,
                    "error",
                    0.0,
                    None,
                );
                return;
            }
        }
        engine.confirm_stage_started(initial_switch.elapsed().as_secs_f64() * sim_speed);
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

        let mut samples_since_flush = 0u32;
        // The control tick is capped independently of the sampling interval: safety checks
        // and stage advancement run at least once a second even when a site configures a
        // slower sampling cadence. A faster configured cadence still samples every tick.
        let control_period = control_period(sample_period);
        let sample_period_s = sample_period.as_secs_f64();
        let mut since_sample = f64::INFINITY; // sample on the very first tick
                                              // Wall-clock driven (#43): the engine is advanced by the time that ACTUALLY
                                              // passed and the next wake-up is a fixed deadline, not "one second after this
                                              // tick's work finished". Counting ticks made every hold stage run long by
                                              // however much the tick's own work (sampling, SQLite, waiting for MFC
                                              // acknowledgements) cost — for GB/T 13241's 180 min reduction hold that is
                                              // neither accurate nor reproducible.
        let mut previous = Instant::now();
        let mut deadline = previous + control_period;

        loop {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let now = Instant::now();
            deadline = next_deadline(deadline, control_period, now);
            let real_dt = now.duration_since(previous).as_secs_f64();
            let dt = real_dt * sim_speed;
            since_sample += real_dt;
            previous = now;

            if exp.stop.load(Ordering::Relaxed) {
                finalize(
                    &app,
                    &hub,
                    &storage,
                    &audit,
                    &exp,
                    &mut buffer,
                    &id,
                    &owner_display_name,
                    "stopped",
                    engine.total_elapsed_s(),
                    None,
                );
                break;
            }

            // Safety (issue #3): sustained loss of a safety-critical bus (5 consecutive
            // failures marks it offline) is an error-exit path — force the protective
            // atmosphere instead of running blind with flammable gas open.
            let mfc_ok = hub.mfc_rx().borrow().is_fresh(Instant::now());
            let temp_ok = hub.temp_rx().borrow().is_fresh(Instant::now());
            if !mfc_ok || !temp_ok {
                let device = if !mfc_ok { "mfc" } else { "temp" };
                audit.write(
                    &AuditEntry::new(
                        Category::Experiment,
                        "device_offline_abort",
                        AuditResult::Failure,
                    )
                    .operator(owner_display_name.clone())
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
                    &owner_display_name,
                    "error",
                    engine.total_elapsed_s(),
                    None,
                );
                break;
            }

            let t8 = hub.temp_rx().borrow().temps[7].unwrap_or(f64::NAN);
            let previous_kind = engine
                .current_stage()
                .map(|s| kind_str(s.kind))
                .unwrap_or("UNKNOWN");
            let mut measured = timed_sample(
                &hub,
                &id,
                w0,
                engine.total_elapsed_s() + dt,
                engine.stage_elapsed_s() + dt,
                previous_kind,
            );
            if let Some(raw) = measured.record_json.as_mut() {
                let mut value: serde_json::Value =
                    serde_json::from_str(raw).expect("sample JSON was just serialized");
                value["stage_index"] = serde_json::json!(engine.stage_index());
                let stable = if engine
                    .current_stage()
                    .is_some_and(|s| t8.is_finite() && (t8 - s.target_temp).abs() <= s.tolerance)
                {
                    engine.stable_elapsed_s() + dt
                } else {
                    0.0
                };
                value["stable_elapsed_s"] = serde_json::json!(stable);
                *raw = value.to_string();
            }
            let action = lock(&exp.stage_action).take();
            let cycle = procedure::tick_cycle(
                &mut engine,
                action.as_ref(),
                &mut lock(&storage),
                &id,
                t8,
                dt,
            );
            let result = match cycle {
                Ok(cycle) => {
                    if let Some(action) = &action {
                        audit.write(
                            &AuditEntry::new(
                                Category::Experiment,
                                "stage_action",
                                if cycle.action_applied {
                                    AuditResult::Success
                                } else {
                                    AuditResult::Failure
                                },
                            )
                            .operator(&action.operator)
                            .detail(serde_json::json!({"action":action,"error":cycle.rejected})),
                        );
                    }
                    if let Some(error) = cycle.rejected {
                        let _ = app.emit("app-notice", format!("阶段操作未执行：{error}"));
                    }
                    cycle.transition
                }
                Err(error) => {
                    buffer.push(measured);
                    audit.write(
                        &AuditEntry::new(
                            Category::Experiment,
                            "stage_action_persistence",
                            AuditResult::Failure,
                        )
                        .operator(&owner_display_name)
                        .detail(serde_json::json!({"id":id,"error":error})),
                    );
                    let _ = app.emit(
                        "app-notice",
                        format!("阶段操作记录保存失败，实验停止：{error}"),
                    );
                    finalize(
                        &app,
                        &hub,
                        &storage,
                        &audit,
                        &exp,
                        &mut buffer,
                        &id,
                        &owner_display_name,
                        "error",
                        engine.total_elapsed_s(),
                        None,
                    );
                    break;
                }
            };

            if since_sample >= sample_period_s || result.apply_stage.is_some() || result.completed {
                buffer.push(measured);
                since_sample = 0.0;
                samples_since_flush += 1;
            }
            if result.apply_stage.is_some() && !exp.stop.load(Ordering::Relaxed) {
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
                if let Some(s) = engine.current_stage() {
                    if s.kind == StageKind::Reducing {
                        let mut baseline =
                            timed_sample(&hub, &id, w0, engine.total_elapsed_s(), 0.0, "REDUCING");
                        if let Some(raw) = baseline.record_json.as_mut() {
                            let mut value: serde_json::Value = serde_json::from_str(raw).unwrap();
                            value["sample_role"] = serde_json::json!("reduction_baseline");
                            *raw = value.to_string();
                        }
                        let saved = lock(&storage).patch_runtime_metadata(&id, &serde_json::json!({
                            "reduction_m1": if baseline.system_message.is_none() { Some(baseline.weight) } else { None }
                        }).to_string());
                        if saved.is_err() || baseline.system_message.is_some() {
                            exp.stop.store(true, Ordering::Relaxed);
                            continue;
                        }
                        buffer.push(baseline);
                    }
                    if apply_stage_atmosphere(&hub, s, w0).is_err() {
                        audit.write(
                            &AuditEntry::new(
                                Category::Experiment,
                                "stage_atmosphere",
                                AuditResult::Failure,
                            )
                            .operator(owner_display_name.clone())
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
                            &owner_display_name,
                            "error",
                            engine.total_elapsed_s(),
                            None,
                        );
                        break;
                    }
                    let confirmed_at = Instant::now();
                    let switching_s = confirmed_at.duration_since(now).as_secs_f64() * sim_speed;
                    let reducing = s.kind == StageKind::Reducing;
                    let stage_name = s.name.clone();
                    engine.confirm_stage_started(switching_s);
                    previous = confirmed_at;
                    deadline = previous + control_period;
                    if reducing && lock(&storage).patch_runtime_metadata(&id,&serde_json::json!({"reduction_start_s":engine.total_elapsed_s(),"gas_switch_confirmed_at":iso_now(),"gas_switch_delay_s":switching_s}).to_string()).is_err() {
                        exp.stop.store(true,Ordering::SeqCst);continue;
                    }
                    audit.write(
                        &AuditEntry::new(Category::Experiment, "stage_enter", AuditResult::Success)
                            .operator(owner_display_name.clone())
                            .detail(serde_json::json!({
                                "id": id, "stage": engine.stage_index(), "name": stage_name
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

            if samples_since_flush >= FLUSH_EVERY_SAMPLES {
                flush(
                    &storage,
                    &audit,
                    &mut buffer,
                    &id,
                    &owner_display_name,
                    result.completed,
                );
                samples_since_flush = 0;
                if buffer.consecutive_failures > 0 {
                    finalize(
                        &app,
                        &hub,
                        &storage,
                        &audit,
                        &exp,
                        &mut buffer,
                        &id,
                        &owner_display_name,
                        "error",
                        engine.total_elapsed_s(),
                        None,
                    );
                    break;
                }
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
                    &owner_display_name,
                    "completed",
                    engine.total_elapsed_s(),
                    ending_gas,
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

#[tauri::command]
fn retry_pending_save(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let user = current_user(&st)?;
    if st.exp.is_running() {
        return Err("请等待实验停止".into());
    }
    let result = recover_unfinished(
        &st.hub,
        &mut lock(&st.storage),
        &st.audit,
        &st.exp,
        &st.recovery_dir,
    );
    st.audit.write(
        &AuditEntry::new(
            Category::Experiment,
            "retry_pending_save",
            if result.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(user.display_name),
    );
    emit_experiment_status(&app, &st.exp.snapshot());
    result
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
    flow_steps: [f64; 4],
}

/// Cadences the UI needs to mirror the backend (docs/06 §6.3), so the staleness threshold
/// and refresh rate come from `comm_config.json` instead of duplicated constants (#44).
#[derive(serde::Serialize)]
struct RuntimeSettings {
    sampling_interval_s: f64,
    telemetry_interval_s: f64,
    heartbeat_timeout_s: f64,
    idle_timeout_s: i64,
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

#[tauri::command(rename_all = "snake_case")]
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
        &AuditEntry::new(Category::Auth, "bootstrap_admin", AuditResult::Success)
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
    let outcome = state
        .auth
        .login(&lock(&state.storage), &username, &password, epoch_now());
    // A rejected login has no session, so "who tried" — the attempted username — is the
    // only attribution available. A successful one uses the authenticated display name, so
    // it matches every other entry in the trail even when the account's display name
    // differs from its username or the operator typed odd casing. Passwords never appear.
    let code = outcome.as_ref().err().map(|error| error.as_str());
    let operator = match outcome.as_ref() {
        Ok(user) => user.display_name.clone(),
        Err(_) => username.clone(),
    };
    state.audit.write(
        &AuditEntry::new(
            Category::Auth,
            "login",
            if outcome.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Rejected
            },
        )
        .operator(operator)
        .detail(serde_json::json!({ "username": username, "code": code })),
    );
    outcome.map_err(auth_error)?;
    get_auth_state(state)
}

#[tauri::command]
fn logout(state: State<AppState>) {
    let operator = audit_operator(&state);
    state.auth.logout();
    state.telemetry_gen.fetch_add(1, Ordering::SeqCst);
    state
        .audit
        .write(&AuditEntry::new(Category::Auth, "logout", AuditResult::Success).operator(operator));
}

#[tauri::command]
fn lock_session(state: State<AppState>) {
    let operator = audit_operator(&state);
    state.auth.lock();
    state.telemetry_gen.fetch_add(1, Ordering::SeqCst);
    state.audit.write(
        &AuditEntry::new(Category::Auth, "lock_session", AuditResult::Success).operator(operator),
    );
}

#[tauri::command]
fn touch_session(state: State<AppState>) -> Result<(), String> {
    state.auth.touch(epoch_now()).map_err(auth_error)
}

#[tauri::command(rename_all = "snake_case")]
fn change_own_password(
    state: State<AppState>,
    current_password: String,
    new_password: String,
) -> Result<(), String> {
    let operator = audit_operator(&state);
    let outcome = state.auth.change_own_password(
        &lock(&state.storage),
        &current_password,
        &new_password,
        epoch_now(),
    );
    state.audit.write(
        &AuditEntry::new(
            Category::Auth,
            "change_password",
            if outcome.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Rejected
            },
        )
        .operator(operator),
    );
    outcome.map_err(auth_error)
}

#[tauri::command]
fn list_users(state: State<AppState>) -> Result<Vec<PublicUser>, String> {
    current_admin(&state)?;
    let users = lock(&state.storage)
        .list_users()
        .map_err(|e| e.to_string())?;
    Ok(users.iter().map(PublicUser::from).collect())
}

#[tauri::command(rename_all = "snake_case")]
fn create_user(
    state: State<AppState>,
    username: String,
    display_name: String,
    temporary_password: String,
    role: UserRole,
) -> Result<PublicUser, String> {
    let actor = current_admin(&state)?;
    let outcome = state.auth.create_user(
        &lock(&state.storage),
        &username,
        &display_name,
        &temporary_password,
        role,
        epoch_now(),
    );
    state.audit.write(
        &AuditEntry::new(
            Category::Auth,
            "create_user",
            if outcome.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Rejected
            },
        )
        .operator(actor.display_name)
        .detail(serde_json::json!({
            "username": username,
            "role": role,
            "user_id": outcome.as_ref().ok().map(|user| user.user_id.clone())
        })),
    );
    let user = outcome.map_err(auth_error)?;
    Ok(PublicUser::from(&user))
}

#[tauri::command(rename_all = "snake_case")]
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
    state.audit.write(
        &AuditEntry::new(Category::Auth, "update_user", AuditResult::Success)
            .operator(actor.display_name)
            .detail(serde_json::json!({
                "user_id": user_id,
                "username": updated.username,
                "role_from": target.role,
                "role_to": updated.role,
                "active_from": target.is_active,
                "active_to": updated.is_active
            })),
    );
    Ok(PublicUser::from(&updated))
}

#[tauri::command(rename_all = "snake_case")]
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
    let outcome = storage.set_user_password(&user_id, &hash, true, &iso_now());
    state.audit.write(
        &AuditEntry::new(
            Category::Auth,
            "reset_password",
            if outcome.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(actor.display_name)
        .detail(serde_json::json!({ "user_id": user_id })),
    );
    outcome.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
fn set_experiment_owner(
    state: State<AppState>,
    experiment_id: String,
    owner_user_id: Option<String>,
) -> Result<(), String> {
    let actor = current_admin(&state)?;
    if state.exp.snapshot().experiment_id.as_deref() == Some(experiment_id.as_str()) {
        return Err("无法变更正在运行实验的归属".into());
    }
    let storage = lock(&state.storage);
    let experiment = storage
        .get_experiment(&experiment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "实验不存在".to_string())?;
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
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(
            Category::Experiment,
            "set_experiment_owner",
            AuditResult::Success,
        )
        .operator(actor.display_name)
        .detail(serde_json::json!({
            "id": experiment_id,
            "old_owner_user_id": experiment.owner_user_id,
            "new_owner_user_id": owner_user_id
        })),
    );
    Ok(())
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        name: "TMH-LPF-900".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[tauri::command]
fn get_runtime_mode(state: State<AppState>) -> Result<String, String> {
    current_user(&state)?;
    Ok(state.mode.clone())
}

#[tauri::command]
fn get_gas_safety_limits(state: State<AppState>) -> Result<GasLimits, String> {
    current_user(&state)?;
    Ok(GasLimits {
        h2: state.limit_h2,
        co: state.limit_co,
        flow_steps: state.flow_steps,
    })
}

#[tauri::command]
fn get_runtime_settings(state: State<AppState>) -> Result<RuntimeSettings, String> {
    current_user(&state)?;
    Ok(RuntimeSettings {
        sampling_interval_s: state.cadence.sampling.as_secs_f64(),
        telemetry_interval_s: state.cadence.telemetry.as_secs_f64(),
        heartbeat_timeout_s: state.cadence.heartbeat_timeout_s,
        idle_timeout_s: state.idle_timeout_s,
    })
}

#[tauri::command]
fn get_device_health(state: State<AppState>) -> Result<DeviceHealthDto, String> {
    current_user(&state)?;
    let t = state.hub.temp_rx().borrow().clone();
    let m = state.hub.mfc_rx().borrow().clone();
    let b = state.hub.balance_rx().borrow().clone();
    Ok(DeviceHealthDto {
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
    })
}

#[tauri::command]
fn get_public_safety_status(state: State<AppState>) -> PublicSafetyStatus {
    PublicSafetyStatus {
        experiment_running: state.exp.is_running(),
        mfc_connected: state.hub.mfc_rx().borrow().is_fresh(Instant::now()),
        temp_connected: state.hub.temp_rx().borrow().is_fresh(Instant::now()),
    }
}

#[tauri::command]
async fn reset_safety_interlock(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let user = current_user(&st)?;
    if st.exp.updating.load(Ordering::SeqCst) {
        return Err("更新期间不能复位保护".into());
    }
    if st.exp.is_running() || st.exp.persistence_pending() {
        return Err("实验仍在结束或数据待保存，暂不能复位".into());
    }
    if !lock(&st.storage)
        .unfinished_experiments()
        .map_err(|e| e.to_string())?
        .is_empty()
    {
        return Err("尚有未保存完成的实验，请先处理数据保存故障".into());
    }
    if !st.hub.temp_rx().borrow().is_fresh(Instant::now()) {
        return Err("温度测量无效，不能复位".into());
    }
    st.hub
        .request_reset_safety()
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let status = ExperimentStatus::idle();
    st.exp.set_status(status.clone());
    emit_experiment_status(&app, &status);
    st.audit.write(
        &AuditEntry::new(
            Category::Gas,
            "reset_safety_interlock",
            AuditResult::Success,
        )
        .operator(user.display_name),
    );
    Ok(())
}

#[tauri::command]
async fn emergency_safe_shutdown(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    // Deliberately callable without a session; still attributed, so the trail shows whether
    // a logged-in operator or someone at a locked screen triggered it (#41).
    let operator = audit_operator(&st);
    let authenticated = st.auth.has_active_session(epoch_now());
    st.exp.stop.store(true, Ordering::Relaxed);
    let result = match st.hub.request_shutoff_flammable().await {
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
        .operator(operator)
        .detail(serde_json::json!({ "authenticated": authenticated })),
    );
    result
}

#[tauri::command]
async fn set_gas_flow(app: AppHandle, gas: usize, value: f64) -> Result<ClampResult, String> {
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
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
                &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Failure)
                    .operator(operator.clone())
                    .detail(
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
                &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Failure)
                    .operator(operator.clone())
                    .detail(
                    serde_json::json!({ "gas": gas, "requested": value, "error": error.to_string() }),
                ),
            );
            return Err("DEVICE_OFFLINE".into());
        }
    };
    st.audit.write(
        &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Success)
            .operator(operator)
            .detail(serde_json::json!({
                "gas": gas, "requested": value, "applied": clamp.applied, "clamped": clamp.clamped
            })),
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
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
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
        .operator(operator)
        .detail(serde_json::json!({ "gas": gas, "on": on })),
    );
    result
}

async fn set_all_valves(
    app: &AppHandle,
    on: bool,
    action: &str,
    operator: String,
) -> Result<(), String> {
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
    app.state::<AppState>().audit.write(
        &AuditEntry::new(
            Category::Gas,
            action,
            if failed {
                AuditResult::Failure
            } else {
                AuditResult::Success
            },
        )
        .operator(operator),
    );
    if failed {
        Err("DEVICE_OFFLINE".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
async fn open_all_valves(app: AppHandle) -> Result<(), String> {
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
    set_all_valves(&app, true, "open_all", operator).await
}

#[tauri::command]
async fn close_all_valves(app: AppHandle) -> Result<(), String> {
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
    set_all_valves(&app, false, "close_all", operator).await
}

#[tauri::command]
async fn shutoff_flammable_gases(app: AppHandle) -> Result<(), String> {
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
    app.state::<AppState>()
        .exp
        .stop
        .store(true, Ordering::SeqCst);
    let hub = app.state::<AppState>().hub.clone();
    let result = match hub.request_shutoff_flammable().await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err("DEVICE_OFFLINE".to_string()),
    };
    app.state::<AppState>().audit.write(
        &AuditEntry::new(
            Category::Gas,
            "shutoff",
            if result.is_ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(operator),
    );
    result
}

#[tauri::command]
async fn tare_balance(app: AppHandle) -> Result<TareResultDto, String> {
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
    let hub = app.state::<AppState>().hub.clone();
    let r = hub
        .request_tare()
        .await
        .map_err(|_| "天平不可用".to_string())?;
    app.state::<AppState>().audit.write(
        &AuditEntry::new(
            Category::Balance,
            "tare",
            if r.success {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(operator),
    );
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
    let auth = st.auth.clone();
    let period = st.cadence.telemetry;
    let my_gen = gen.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let mut seq = 0u64;
        loop {
            std::thread::sleep(period);
            if gen.load(Ordering::SeqCst) != my_gen || !auth.has_active_session(epoch_now()) {
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

#[derive(serde::Serialize)]
struct PreflightIssue {
    code: String,
    message_zh: String,
}

impl PreflightIssue {
    fn blocking(code: &str, message_zh: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message_zh: message_zh.into(),
        }
    }

    fn warning(code: &str, message_zh: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message_zh: message_zh.into(),
        }
    }
}

#[derive(serde::Serialize)]
struct ExperimentPreflightResult {
    ok: bool,
    blocking: Vec<PreflightIssue>,
    warnings: Vec<PreflightIssue>,
    program: Option<Program>,
}

#[derive(serde::Serialize)]
struct ExperimentStartDefaults {
    sample_id: String,
    sample_name: String,
    sample_weight: Option<f64>,
    operator: String,
    experiment_type: String,
    project_name: Option<String>,
    experiment_time: String,
    notes: Option<String>,
}

fn compact_date(value: &str) -> String {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 8 {
        digits[..8].to_string()
    } else {
        chrono::Local::now().format("%Y%m%d").to_string()
    }
}

fn next_sample_id(program: &Program, experiment_time: &str, rows: &[ExperimentRow]) -> String {
    let prefix = normalize_sample_prefix(&program.sample_prefix, &program.id);
    let stem = format!("{}-{}-", prefix, compact_date(experiment_time));
    let max_seq = rows
        .iter()
        .filter_map(|r| r.sample_id.as_deref())
        .filter_map(|id| id.strip_prefix(&stem))
        .filter_map(|suffix| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{stem}{:03}", max_seq + 1)
}

// ---- Experiment -------------------------------------------------------------

#[tauri::command]
fn get_experiment_state(state: State<AppState>) -> Result<ExperimentStatus, String> {
    let user = current_user(&state)?;
    Ok(redact_experiment_status(
        state.exp.snapshot(),
        user.role,
        &user.user_id,
    ))
}

/// Validate experiment start parameters (docs/05 §5.2). Pure so it is unit-testable.
fn validate_experiment_params(params: &ExperimentParams) -> Result<(), String> {
    if params.sample_id.trim().is_empty() {
        return Err("样品编号必填".into());
    }
    if params.sample_name.trim().is_empty() {
        return Err("样品名称必填".into());
    }
    if !params.sample_weight.is_finite() || params.sample_weight <= 0.0 {
        return Err("样品重量必须大于 0".into());
    }
    if !STANDARD_MODES.contains(&params.experiment_type.as_str())
        && !params.experiment_type.starts_with("custom-")
    {
        return Err("未知实验模式".into());
    }
    Ok(())
}

fn build_preflight(state: &AppState, params: &ExperimentParams) -> ExperimentPreflightResult {
    let mut blocking = Vec::new();
    if state.hub.safety_latched() || state.exp.snapshot().phase == Phase::Error {
        blocking.push(PreflightIssue::blocking(
            "SAFETY_LATCHED",
            "保护已锁存；请确认气体测量、设备与数据保存后复位",
        ));
    }
    if let Err(message) = validate_experiment_params(params) {
        blocking.push(PreflightIssue::blocking("PARAMS", message));
    }
    let mut warnings = Vec::new();

    if state.exp.updating.load(Ordering::SeqCst) {
        blocking.push(PreflightIssue::blocking(
            "UPDATE_IN_PROGRESS",
            "正在安装更新，不能开始实验",
        ));
    }

    if state.exp.persistence_pending() {
        blocking.push(PreflightIssue::blocking(
            "PERSISTENCE_PENDING",
            "存在待恢复实验数据，请先重试保存",
        ));
    }
    if state.exp.is_running() {
        blocking.push(PreflightIssue::blocking(
            "EXPERIMENT_RUNNING",
            "实验已在运行",
        ));
    }

    let program = match program_by_id(state, &params.experiment_type) {
        Ok(p) => {
            if let Err(message) = validate_executable_program(&p) {
                blocking.push(PreflightIssue::blocking("PROGRAM_INVALID", message));
            }
            if !p.enabled {
                blocking.push(PreflightIssue::blocking(
                    "PROGRAM_DISABLED",
                    "实验模式已停用，无法启动",
                ));
            }
            if let Err(message) = custom_programs::validate_gas(state, &p) {
                blocking.push(PreflightIssue::blocking("PROGRAM_GAS_INVALID", message));
            }
            Some(p)
        }
        Err(e) => {
            blocking.push(PreflightIssue::blocking("PROGRAM_NOT_FOUND", e));
            None
        }
    };

    let temp = state.hub.temp_rx().borrow().clone();
    let mfc = state.hub.mfc_rx().borrow().clone();
    let balance = state.hub.balance_rx().borrow().clone();
    if !temp.is_fresh(Instant::now()) || !temp.temps[7].is_some_and(|v| v.is_finite()) {
        blocking.push(PreflightIssue::blocking(
            "TEMP_OFFLINE",
            "温度总线离线，无法启动实验",
        ));
    }
    if !mfc.is_fresh(Instant::now()) {
        blocking.push(PreflightIssue::blocking(
            "MFC_OFFLINE",
            "气体流量 MFC 离线，无法启动实验",
        ));
    }
    if !balance.is_fresh(Instant::now()) || !balance.weight.is_some_and(|v| v.is_finite()) {
        blocking.push(PreflightIssue::blocking(
            "BALANCE_OFFLINE",
            "电子天平离线，无法启动实验",
        ));
    }
    if let Some(weight) = balance.weight {
        if params.sample_weight > 0.0 {
            let diff = (weight - params.sample_weight).abs();
            let threshold = (params.sample_weight * 0.02).max(0.1);
            if diff > threshold {
                warnings.push(PreflightIssue::warning(
                    "WEIGHT_MISMATCH",
                    format!(
                        "当前天平读数 {:.3} g 与输入样品重量 {:.3} g 差异较大，请确认初始重量",
                        weight, params.sample_weight
                    ),
                ));
            }
        }
    } else if balance.connected {
        warnings.push(PreflightIssue::warning(
            "BALANCE_NO_WEIGHT",
            "电子天平在线但暂无有效重量读数，请确认初始重量",
        ));
    }

    ExperimentPreflightResult {
        ok: blocking.is_empty(),
        blocking,
        warnings,
        program,
    }
}

#[tauri::command]
fn preflight_experiment(
    state: State<AppState>,
    params: ExperimentParams,
) -> Result<ExperimentPreflightResult, String> {
    current_user(&state)?;
    Ok(build_preflight(&state, &params))
}

#[tauri::command(rename_all = "snake_case")]
fn generate_sample_id(
    state: State<AppState>,
    program_id: String,
    experiment_time: String,
) -> Result<String, String> {
    let program = program_by_id(&state, &program_id)?;
    let user = current_user(&state)?;
    let rows = visible_rows(&lock(&state.storage), &user)?;
    Ok(next_sample_id(&program, &experiment_time, &rows))
}

#[tauri::command(rename_all = "snake_case")]
fn get_experiment_start_defaults(
    state: State<AppState>,
    program_id: String,
) -> Result<ExperimentStartDefaults, String> {
    let program = program_by_id(&state, &program_id)?;
    let now = iso_now();
    let user = current_user(&state)?;
    let rows = visible_rows(&lock(&state.storage), &user)?;
    let latest = rows.first().cloned();
    let balance = state.hub.balance_rx().borrow().clone();
    Ok(ExperimentStartDefaults {
        sample_id: next_sample_id(&program, &now, &rows),
        sample_name: latest
            .as_ref()
            .map(|r| r.sample_name.clone())
            .unwrap_or_default(),
        sample_weight: balance
            .weight
            .filter(|w| balance.is_fresh(Instant::now()) && w.is_finite() && *w > 0.0),
        operator: user.display_name,
        experiment_type: program_id,
        project_name: latest.and_then(|r| r.project_name),
        experiment_time: now,
        notes: None,
    })
}

fn visible_rows(storage: &Storage, user: &UserRow) -> Result<Vec<ExperimentRow>, String> {
    if user.role == UserRole::Admin {
        storage.list_experiments()
    } else {
        storage.list_experiments_for_owner(&user.user_id)
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn start_experiment(app: AppHandle, params: ExperimentParams) -> Result<String, String> {
    validate_experiment_params(&params)?;

    let st = app.state::<AppState>();
    let _lifecycle = lock(&st.exp.lifecycle);
    let user = current_user(&st)?;
    if st.exp.is_running() {
        return Err("实验已在运行".into());
    }
    let preflight = build_preflight(&st, &params);
    if let Some(issue) = preflight.blocking.first() {
        return Err(issue.message_zh.clone());
    }
    let program = preflight.program.ok_or("未知实验模式")?;
    validate_executable_program(&program)?;

    let now = iso_now();
    let id = format!("exp-{}", tmh_core::auth::new_user_id());
    let row = ExperimentRow {
        experiment_id: id.clone(),
        experiment_name: params.sample_name.clone(),
        sample_id: Some(params.sample_id.trim().to_string()),
        project_name: clean_opt(params.project_name.clone()),
        experiment_time: Some(now.clone()),
        program_snapshot_json: Some(serde_json::to_string(&program).map_err(|e| e.to_string())?),
        sample_name: params.sample_name.clone(),
        sample_weight: params.sample_weight,
        start_time: now.clone(),
        end_time: None,
        description: clean_opt(params.notes.clone()),
        operator: Some(user.display_name.clone()),
        experiment_type: Some(params.experiment_type.clone()),
        analysis_results_json: None,
        created_at: now.clone(),
        owner_user_id: Some(user.user_id.clone()),
    };
    lock(&st.storage)
        .insert_experiment(&row)
        .map_err(|e| e.to_string())?;

    lock(&st.storage)
        .patch_runtime_metadata(
            &id,
            &serde_json::json!({
                "schema_version":1,"outcome":"running","device_mode":st.mode,
                "sample_interval_s":st.cadence.sampling.as_secs_f64()*st.sim_speed,
                "calculation_version":CALCULATION_VERSION,
                "gas_safety_limits":{"h2":st.limit_h2,"co":st.limit_co,"flow_steps":st.flow_steps}
            })
            .to_string(),
        )
        .map_err(|e| e.to_string())?;

    // Reset signals and publish the initial running status before the task spins up.
    st.exp.stop.store(false, Ordering::Relaxed);
    *lock(&st.exp.stage_action) = None;
    let first = program.stages.first();
    let initial = ExperimentStatus {
        updating: false,
        persistence_pending: false,
        running: true,
        experiment_id: Some(id.clone()),
        mode: Some(params.experiment_type.clone()),
        sample_name: Some(params.sample_name.clone()),
        started_at: Some(now.clone()),
        phase: Phase::StageTransition,
        stage_index: 0,
        total_stages: program.stages.len(),
        stage_name: first.map(|s| s.name.clone()),
        stage_kind: first.map(|s| s.kind),
        target_temp: first.map(|s| s.target_temp).unwrap_or(0.0),
        tolerance: first.map(|s| s.tolerance).unwrap_or(0.0),
        stage_elapsed_s: 0.0,
        stable_elapsed_s: 0.0,
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
    emit_experiment_status(&app, &initial);

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
        st.cadence.sampling,
    );
    Ok(id)
}

#[tauri::command]
async fn stop_experiment(app: AppHandle, reason: String) -> Result<(), String> {
    let operator = require_experiment_controller(&app.state::<AppState>())?.display_name;
    let (hub, audit, exp) = {
        let st = app.state::<AppState>();
        (st.hub.clone(), st.audit.clone(), st.exp.clone())
    };
    // Signal the tick task to finalize (it publishes idle within ≤1s).
    exp.stop.store(true, Ordering::Relaxed);
    // HARD INVARIANT (docs/05 §5.5): apply the safety atmosphere immediately here too —
    // belt-and-suspenders even if the tick task has already died.
    let safety_result = match hub.request_shutoff_flammable().await {
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
        .operator(operator)
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
        emit_experiment_status(&app, &status);
    }
    safety_result
}

#[tauri::command(rename_all = "snake_case")]
fn skip_stage(
    state: State<AppState>,
    experiment_id: String,
    stage_index: usize,
    reason: String,
) -> Result<(), String> {
    procedure::queue_action(
        &state,
        experiment_id,
        stage_index,
        procedure::ActionKind::Skip,
        reason,
    )
}

#[tauri::command(rename_all = "snake_case")]
fn acknowledge_stage(
    state: State<AppState>,
    experiment_id: String,
    stage_index: usize,
    notes: String,
) -> Result<(), String> {
    procedure::queue_action(
        &state,
        experiment_id,
        stage_index,
        procedure::ActionKind::Confirm,
        notes,
    )
}

/// The three GB standard programs (docs/05 §5.3) for the experiment-mode page.
#[tauri::command]
fn list_programs(state: State<AppState>) -> Result<Vec<Program>, String> {
    current_user(&state)?;
    all_programs(&lock(&state.storage))
}

#[tauri::command]
fn save_custom_program(
    state: State<AppState>,
    program: Program,
    reason: String,
) -> Result<Program, String> {
    custom_programs::save(&state, program, &reason)
}

#[tauri::command]
fn list_custom_program_revisions(
    state: State<AppState>,
    id: String,
) -> Result<Vec<Program>, String> {
    custom_programs::history(&state, &id)
}

#[tauri::command]
fn delete_custom_program(state: State<AppState>, id: String) -> Result<(), String> {
    custom_programs::delete(&state, &id)
}

// ---- History query ----------------------------------------------------------

#[derive(serde::Serialize)]
struct ExperimentSummary {
    sample_id: Option<String>,
    project_name: Option<String>,
    experiment_time: Option<String>,
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
        sample_id: r.sample_id,
        project_name: r.project_name,
        experiment_time: r.experiment_time,
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
    revisions: Vec<serde_json::Value>,
    runtime: serde_json::Value,
    program_snapshot: Option<Program>,
    sample_id: Option<String>,
    project_name: Option<String>,
    experiment_time: Option<String>,
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
        revisions: Vec::new(),
        runtime: serde_json::json!({}),
        program_snapshot: r
            .program_snapshot_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok()),
        experiment_id: r.experiment_id,
        experiment_name: r.experiment_name,
        sample_id: r.sample_id,
        project_name: r.project_name,
        experiment_time: r.experiment_time,
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
    quality: Option<String>,
    record: Option<serde_json::Value>,
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
        quality: r.system_message,
        record: r
            .record_json
            .as_deref()
            .and_then(|v| serde_json::from_str(v).ok()),
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
    let mut detail = detail_of(row, count);
    detail.runtime = serde_json::from_str(&s.runtime_metadata(&id).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    detail.revisions = s.analysis_revisions(&id).map_err(|e| e.to_string())?;
    Ok(detail)
}

#[tauri::command(rename_all = "snake_case")]
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
    let actor = current_admin(&state)?;
    if state.exp.persistence_pending() {
        return Err("存在尚未保存的数据，请先重试保存".into());
    }
    // Never delete the run currently in progress.
    if state.exp.snapshot().experiment_id.as_deref() == Some(id.as_str()) {
        return Err("无法删除正在运行的实验".into());
    }
    let mut storage = lock(&state.storage);
    if storage
        .get_experiment(&id)
        .map_err(|e| e.to_string())?
        .is_some_and(|r| r.end_time.is_none())
    {
        return Err("实验尚未完成保存，不能删除".into());
    }
    storage.delete_experiment(&id).map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(Category::Experiment, "delete", AuditResult::Success)
            .operator(actor.display_name)
            .detail(serde_json::json!({ "id": id })),
    );
    Ok(())
}

// ---- GB analysis ------------------------------------------------------------

/// Read exact stage time without treating old untimed rows or missing readings as zero.
fn reduction_curve(
    samples: &[SampleRow],
    m0: f64,
    m1: f64,
    input: &ReductionInput,
    max_gap_s: f64,
) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::new();
    for row in samples
        .iter()
        .filter(|r| r.experiment_status.as_deref() == Some("REDUCING"))
    {
        let record: serde_json::Value =
            serde_json::from_str(row.record_json.as_deref().unwrap_or("{}")).unwrap_or_default();
        let Some(t) = record["stage_elapsed_s"].as_f64() else {
            continue;
        };
        let rt = if row.system_message.is_none() {
            reducibility(m0, m1, row.weight, input.w1, input.w2).map(|r| r.rt)
        } else {
            None
        };
        if let Some(&(previous, _)) = points.last() {
            if (t / 60.0 - previous) * 60.0 > max_gap_s {
                points.push(((previous + t / 60.0) / 2.0, f64::NAN));
            }
        }
        points.push((t / 60.0, rt.unwrap_or(f64::NAN)));
    }
    points
}
fn value_at(points: &[(f64, f64)], time: f64) -> Option<f64> {
    if let Some((_, v)) = points.iter().find(|p| p.0 == time && p.1.is_finite()) {
        return Some(*v);
    }
    points.windows(2).find_map(|p| {
        let ((a, b), (c, d)) = (p[0], p[1]);
        (a < time && c > time && b.is_finite() && d.is_finite())
            .then(|| b + (d - b) * (time - a) / (c - a))
    })
}

/// Persist an analysis-result JSON onto the experiment row and audit it.
fn persist_analysis(
    state: &AppState,
    id: &str,
    standard: &str,
    operator: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let mut versioned = value.clone();
    versioned["computed_by"] = serde_json::json!(operator);
    let json = serde_json::to_string(&versioned).map_err(|e| e.to_string())?;
    lock(&state.storage)
        .set_analysis_results(id, &json)
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(Category::Experiment, "analyze", AuditResult::Success)
            .operator(operator.to_string())
            .detail(serde_json::json!({ "id": id, "standard": standard })),
    );
    Ok(())
}

/// Authorise access to one experiment and return the session user (needed for audit
/// attribution at the call site).
fn require_experiment(state: &AppState, id: &str) -> Result<UserRow, String> {
    let user = current_user(state)?;
    authorize_experiment(state, id, &user)?;
    Ok(user)
}

fn analysis_context(
    state: &AppState,
    id: &str,
    expected: &str,
) -> Result<(UserRow, ExperimentRow, Vec<SampleRow>, serde_json::Value), String> {
    let user = require_experiment(state, id)?;
    let db = lock(&state.storage);
    let row = db
        .get_experiment(id)
        .map_err(|e| e.to_string())?
        .ok_or("实验不存在")?;
    if row.end_time.is_none() {
        return Err("实验未结束，暂不可分析".into());
    }
    let program: Option<Program> = row
        .program_snapshot_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let method = program
        .as_ref()
        .and_then(|p| p.base_standard_id.as_deref())
        .or(row.experiment_type.as_deref());
    if method != Some(expected) {
        return Err("分析方法与本次执行程序不匹配".into());
    }
    let meta = serde_json::from_str(&db.runtime_metadata(id).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok((
        user,
        row,
        db.get_samples(id, 0).map_err(|e| e.to_string())?,
        meta,
    ))
}

#[tauri::command]
fn analyze_gb13241(
    state: State<AppState>,
    id: String,
    input: ReductionInput,
) -> Result<serde_json::Value, String> {
    let (user, row, samples, meta) = analysis_context(&state, &id, "REDUCIBILITY")?;
    let value = calculate_reduction(&row, &samples, &meta, &input)?;
    persist_analysis(&state, &id, "GB/T 13241", &user.display_name, &value)?;
    Ok(value)
}
fn reduction_endpoint(row: &ExperimentRow) -> Result<(f64, bool), String> {
    let Some(raw) = &row.program_snapshot_json else {
        if row
            .experiment_type
            .as_deref()
            .is_some_and(|id| id.starts_with("custom-"))
        {
            return Err("缺少自定义程序快照，不能确定还原终点".into());
        }
        return Ok((180.0, false));
    };
    let program: Program = serde_json::from_str(raw).map_err(|_| "执行程序快照损坏")?;
    let stages: Vec<_> = program
        .stages
        .iter()
        .filter(|s| s.kind == StageKind::Reducing)
        .collect();
    if stages.len() != 1 || !stages[0].duration_min.is_finite() || stages[0].duration_min <= 0.0 {
        return Err("还原段时长或数量不受当前分析方法支持".into());
    }
    Ok((
        stages[0].duration_min,
        program.source == ProgramSource::Custom,
    ))
}

fn calculate_reduction(
    row: &ExperimentRow,
    samples: &[SampleRow],
    meta: &serde_json::Value,
    input: &ReductionInput,
) -> Result<serde_json::Value, String> {
    let (endpoint_min, custom) = reduction_endpoint(row)?;
    if input.manual_m180.is_some() && (endpoint_min != 180.0 || input.manual_mt.is_some()) {
        return Err("m₁₈₀ 仅适用于 180 分钟终点且不能与 mₜ 重复补录".into());
    }
    let manual_used = input.manual_m1.is_some()
        || input.manual_mt.is_some()
        || input.manual_m180.is_some()
        || input.manual_rate_points.is_some();
    if manual_used
        && input
            .source_record
            .as_ref()
            .map_or(true, |v| v.trim().is_empty())
    {
        return Err("人工补录须填写原始记录编号或来源说明".into());
    }
    let m1 = input
        .manual_m1
        .or_else(|| meta["reduction_m1"].as_f64())
        .ok_or("缺少还原开始前有效总称量 m₁，可依据原始记录人工补录")?;
    let max_gap = meta["sample_interval_s"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(0.0)
        * 1.5;
    reducibility(row.sample_weight, m1, m1, input.w1, input.w2).ok_or("亚铁/全铁含量或称量无效")?;
    let points = reduction_curve(samples, row.sample_weight, m1, input, max_gap);
    let rt = if let Some(mt) = input.manual_mt.or(input.manual_m180) {
        reducibility(row.sample_weight, m1, mt, input.w1, input.w2)
            .ok_or("还原终点总称量无效")?
            .rt
    } else {
        value_at(&points, endpoint_min).ok_or_else(|| {
            format!("{endpoint_min} 分钟处没有连续有效称量，可依据原始记录人工补录 mₜ")
        })?
    };
    let rate = if let Some(manual) = &input.manual_rate_points {
        if manual.iter().any(|(t, _)| *t > endpoint_min) {
            return Err("人工速率曲线时间不能超出本次程序的还原终点".into());
        }
        Some(
            reduction_rate(manual)
                .ok_or("人工速率曲线须按时间递增并覆盖 30% 与 60%、55% 或 50% 还原度")?,
        )
    } else {
        reduction_rate(&points)
    };
    let mut value = serde_json::json!({"standard":"GB/T 13241","standard_version":"2017","calculation_version":CALCULATION_VERSION,
      "manual_used":manual_used,"input":input,"m0":row.sample_weight,"m1":m1,"rt":rt,"endpoint_min":endpoint_min,
      "reduction_rate_index":rate.as_ref().map(|r|r.index),"rate_evidence":rate,
      "runtime_evidence":meta,"result_scope":"single_trial","standard_conformity":"pending_group_and_process_verification","computed_at":iso_now()});
    if custom {
        value["standard_conformity"] = serde_json::json!("custom_conditions");
    } else if endpoint_min == 180.0 {
        value["ri"] = serde_json::json!(rt);
    }
    Ok(value)
}

#[tauri::command]
fn analyze_gb13242(
    state: State<AppState>,
    id: String,
    input: RdiInput,
) -> Result<serde_json::Value, String> {
    let (user, _, _, meta) = analysis_context(&state, &id, "LOW_TEMP_DEGRADATION")?;
    let result = rdi(input.m0, input.m1, input.m2, input.m3)
        .ok_or("入鼓质量或筛上质量无效：须为有限非负值且筛上合计不大于入鼓质量")?;
    if !input.drum_rpm.is_finite()
        || input.drum_rpm <= 0.0
        || input.drum_revolutions == 0
        || !input.sieve_seconds.is_finite()
        || input.sieve_seconds <= 0.0
    {
        return Err("请填写实际转鼓与筛分记录".into());
    }
    let external_conditions_met = (input.drum_rpm - 30.0).abs() <= 1.0
        && input.drum_revolutions == 300
        && input.sieve_seconds == 60.0;
    let value = serde_json::json!({"standard":"GB/T 13242","standard_version":"2017","calculation_version":CALCULATION_VERSION,
      "input":input,"rdi_plus_6_3":result.rdi_plus_6_3,"rdi_plus_3_15":result.rdi_plus_3_15,"rdi_minus_0_5":result.rdi_minus_0_5,
      "external_conditions_met":external_conditions_met,"runtime_evidence":meta,"result_scope":"single_trial","standard_conformity":"pending_group_and_process_verification","computed_at":iso_now()});
    persist_analysis(&state, &id, "GB/T 13242", &user.display_name, &value)?;
    Ok(value)
}
#[tauri::command]
fn analyze_gb13240(
    state: State<AppState>,
    id: String,
    input: SwellingInput,
) -> Result<serde_json::Value, String> {
    let (user, _, _, meta) = analysis_context(&state, &id, "FREE_SWELLING")?;
    if input.pellet_count != 18 || input.volume_method.trim().is_empty() || !input.same_method {
        return Err("须为同一份 18 颗球团的总体积，并确认还原前后采用相同体积测定方法".into());
    }
    let result = free_swelling(&[(input.v0, input.v1)]).ok_or("整份体积必须为有限正值")?;
    let value = serde_json::json!({"standard":"GB/T 13240","standard_version":"2018","calculation_version":CALCULATION_VERSION,
      "input":input,"swelling_index":result.swelling_index,"runtime_evidence":meta,
      "result_scope":"single_trial","standard_conformity":"pending_group_and_process_verification","computed_at":iso_now()});
    persist_analysis(&state, &id, "GB/T 13240", &user.display_name, &value)?;
    Ok(value)
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ReportDetails {
    lab_name: String,
    lab_address: String,
    report_number: String,
    report_date: String,
    sample_preparation: String,
    equipment: String,
    gas_and_calibration: String,
    external_steps: String,
    deviations: String,
    reviewer: String,
    repeat_group_reference: String,
}
#[tauri::command]
fn save_report_details(
    state: State<AppState>,
    id: String,
    details: ReportDetails,
) -> Result<(), String> {
    let user = require_experiment(&state, &id)?;
    let value = serde_json::to_value(&details).map_err(|e| e.to_string())?;
    if value
        .as_object()
        .unwrap()
        .values()
        .any(|v| v.as_str().map_or(true, |s| s.len() > 4096))
    {
        return Err("报告字段过长".into());
    }
    let patch = serde_json::json!({"report_details":details,"report_details_operator":user.display_name,"report_details_updated_at":iso_now()});
    lock(&state.storage)
        .save_report_revision(&id, &patch.to_string())
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(Category::Experiment, "report_details", AuditResult::Success)
            .operator(user.display_name)
            .detail(serde_json::json!({"id":id})),
    );
    Ok(())
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
    let mut analysis = exp
        .analysis_results_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok());
    let runtime: serde_json::Value = serde_json::from_str(
        &lock(&st.storage)
            .runtime_metadata(&id)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let a = analysis.get_or_insert_with(|| serde_json::json!({}));
    a["report_details"] = runtime["report_details"].clone();
    a["runtime_evidence"] = runtime;
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
        &AuditEntry::new(Category::Export, "generate_report", AuditResult::Success)
            .operator(user.display_name)
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
        &AuditEntry::new(Category::Export, "export_data", AuditResult::Success)
            .operator(user.display_name)
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
fn get_updater_status(app: AppHandle) -> Result<UpdateCheck, String> {
    current_user(&app.state::<AppState>())?;
    let enabled = app.state::<AppState>().updater_enabled;
    Ok(UpdateCheck::base(
        enabled,
        app.package_info().version.to_string(),
    ))
}

/// Check the manifest endpoint for a newer version. Gated; never errors into the UI —
/// an unreachable endpoint yields a friendly `error` string (offline is normal here).
#[tauri::command]
async fn check_for_update(app: AppHandle) -> Result<UpdateCheck, String> {
    let operator = current_user(&app.state::<AppState>())?.display_name;
    let current = app.package_info().version.to_string();
    let (enabled, audit) = {
        let st = app.state::<AppState>();
        (st.updater_enabled, st.audit.clone())
    };
    let mut check = UpdateCheck::base(enabled, current);
    if !enabled {
        return Ok(check);
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
        .operator(operator)
        .detail(
            serde_json::json!({ "available": check.available, "new_version": check.new_version }),
        ),
    );
    let _ = app.emit("update-status", &check);
    Ok(check)
}

/// Download + verify (minisign) + install the update, then restart. Gated.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let operator = current_admin(&st)?.display_name;
    if !st.updater_enabled {
        return Err("更新功能未启用".into());
    }
    let lease = UpdateLease::begin(st.exp.clone())?;
    emit_experiment_status(&app, &st.exp.snapshot());
    let result = perform_update(&app, operator).await;
    drop(lease);
    emit_experiment_status(&app, &st.exp.snapshot());
    result
}

struct UpdateLease(Arc<ExpControl>);
impl UpdateLease {
    fn begin(exp: Arc<ExpControl>) -> Result<Self, String> {
        {
            let _lifecycle = lock(&exp.lifecycle);
            if exp.is_running() || exp.persistence_pending() || exp.updating.load(Ordering::SeqCst)
            {
                return Err("请先停止实验并完成数据保存，且不能重复安装更新".into());
            }
            exp.updating.store(true, Ordering::SeqCst);
        }
        Ok(Self(exp))
    }
}
impl Drop for UpdateLease {
    fn drop(&mut self) {
        self.0.updating.store(false, Ordering::SeqCst);
    }
}

async fn perform_update(app: &AppHandle, operator: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    // Latch before the first await: no queued/manual gas command may reopen gas
    // while the installer runs; update restart bypasses Tauri's prevent_exit.
    st.hub
        .request_shutoff_flammable()
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("已是最新版本")?;
    if st.exp.is_running() || st.exp.persistence_pending() {
        return Err("实验或保存尚未结束".into());
    }
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    if st.exp.is_running() || st.exp.persistence_pending() {
        return Err("数据未保存，不能重启".into());
    }
    st.audit.write(
        &AuditEntry::new(Category::App, "install_update", AuditResult::Success).operator(operator),
    );
    app.restart();
}

// ---- App entrypoint ---------------------------------------------------------

/// Return true while stopping, protecting, or saving still requires a live window.
fn prepare_exit(app: &AppHandle) -> bool {
    let Some(st) = app.try_state::<AppState>() else {
        return false;
    };
    st.exp.stop.store(true, Ordering::SeqCst);
    let protection = apply_safety_atmosphere_blocking(&st.hub);
    let blocked = protection.is_err()
        || st.exp.is_running()
        || st.exp.persistence_pending()
        || st.exp.updating.load(Ordering::SeqCst);
    if blocked {
        let message = if st.exp.updating.load(Ordering::SeqCst) {
            "正在安装更新，请等待安装结束。"
        } else if st.exp.persistence_pending() {
            "数据尚未保存，已阻止退出。请在实验栏重试保存后再关闭。"
        } else if protection.is_err() {
            "保护气氛未确认，已阻止退出。请检查设备并重试保护操作。"
        } else {
            "正在停止实验并保存数据，请待实验停止后再次关闭。"
        };
        let _ = app.emit("app-notice", message);
    }
    st.audit.write(&AuditEntry::new(
        Category::App,
        "exit",
        if blocked {
            AuditResult::Failure
        } else {
            AuditResult::Success
        },
    ));
    blocked
}

/// Called only after single-instance plugin initialization.
fn initialize_state() -> AppState {
    let mode = std::env::var("TMH_DEVICE_MODE").unwrap_or_else(|_| "mock".into());
    if let Err(message) = validate_runtime_mode(&mode) {
        eprintln!("TMH 启动失败: {message}");
        std::process::exit(1);
    }
    let sim_speed = sim_speed(&mode);
    // Software update checking is OFF by default (docs/09 §9.3); config/env enables it.
    let updater_enabled = matches!(
        std::env::var("TMH_UPDATER").as_deref(),
        Ok("1" | "on" | "true")
    );
    // Persistent per-user data dir (Windows %APPDATA%, Linux ~/.local/share, etc.);
    // TMH_DATA_DIR selects an isolated test profile. Never silently store durable
    // experiment history in a temporary directory.
    let data_root = std::env::var_os("TMH_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join("tmh-lpf-900")))
        .expect("无法确定持久数据目录，请设置 TMH_DATA_DIR");
    let audit_dir = data_root.join("logs");
    let data_lock = recovery::DataDirectoryLock::acquire(&data_root).expect("无法独占实验数据目录");
    let db_dir = data_root.join("data");
    let exports_dir = data_root.join("exports");
    let recovery_dir = data_root.join("recovery");
    let exp = Arc::new(ExpControl::new());
    let _ = std::fs::create_dir_all(&audit_dir);
    let _ = std::fs::create_dir_all(&db_dir);
    let _ = std::fs::create_dir_all(&exports_dir);
    // Pay the Argon2 cost once here, before any login can be attempted.
    warm_password_hashing();
    let mut storage = Storage::open(&db_dir.join("experiments.db")).expect("open experiments db");
    let comm_config = load_comm_config();
    let hub = Arc::new(build_mock_hub(&comm_config));
    let audit = Arc::new(AuditLog::new(audit_dir));
    // Enforce the documented 180-day audit retention (docs/06 §6.5) at startup — issue #6.
    audit.prune(180);

    // Crash recovery before the UI comes up (safety atmosphere for any open run).
    let _ = recover_unfinished(&hub, &mut storage, &audit, &exp, &recovery_dir);

    let idle_timeout_s = idle_timeout_s();
    AppState {
        _data_lock: data_lock,
        recovery_dir,
        hub,
        mode,
        limit_h2: comm_config
            .gas_safety_limits
            .h2
            .min(tmh_core::clamp::H2_ABS_MAX),
        limit_co: comm_config
            .gas_safety_limits
            .co
            .min(tmh_core::clamp::CO_ABS_MAX),
        flow_steps: custom_programs::flow_steps(&comm_config),
        audit,
        storage: Arc::new(Mutex::new(storage)),
        auth: Arc::new(AuthService::new(idle_timeout_s)),
        exp,
        sim_speed,
        exports_dir,
        updater_enabled,
        telemetry_gen: Arc::new(AtomicU64::new(0)),
        cadence: Cadence::from_config(&comm_config),
        idle_timeout_s,
    }
}

/// Build and run the Tauri application.
pub fn run() {
    // Low-end 工控核显 escape hatch (docs/09 §9.1): disable GPU accel on demand.
    if std::env::var("TMH_DISABLE_GPU").is_ok() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-gpu");
    }
    tauri::Builder::default()
        // Single-instance MUST be registered first (serial-port exclusivity, docs/03).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            if let Some(state) = app.try_state::<AppState>() {
                state.audit.write(&AuditEntry::new(
                    Category::App,
                    "start_blocked",
                    AuditResult::Rejected,
                ));
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if prepare_exit(window.app_handle()) {
                    api.prevent_close();
                }
            }
        })
        .setup(|app| {
            app.manage(initialize_state());
            Ok(())
        })
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
            reset_safety_interlock,
            retry_pending_save,
            get_app_info,
            get_runtime_mode,
            get_gas_safety_limits,
            get_runtime_settings,
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
            acknowledge_stage,
            list_programs,
            preflight_experiment,
            generate_sample_id,
            get_experiment_start_defaults,
            save_custom_program,
            list_custom_program_revisions,
            delete_custom_program,
            list_experiments,
            get_experiment_detail,
            get_experiment_samples,
            delete_experiment,
            analyze_gb13241,
            analyze_gb13242,
            analyze_gb13240,
            generate_report,
            save_report_details,
            export_experiment,
            get_updater_status,
            check_for_update,
            install_update
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if prepare_exit(app) {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmh_core::auth::UserRole;

    /// A real `AppState` on an in-memory database and a mock hub. The authorization helpers
    /// below (`current_user`, `current_admin`, `authorize_experiment`,
    /// `require_experiment_controller`) are the single enforcement point every protected
    /// command funnels through, so exercising them here covers the matrix the spec promises
    /// even though the `#[tauri::command]` wrappers themselves need a running app (#45).
    pub(super) fn test_state() -> AppState {
        let cfg = CommConfig::from_json(DEFAULT_COMM_CONFIG).expect("built-in config");
        let dir = std::env::temp_dir().join(format!("tmh_state_{}", tmh_core::auth::new_user_id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        AppState {
            _data_lock: recovery::DataDirectoryLock::acquire(&dir).unwrap(),
            recovery_dir: dir.join("recovery"),
            hub: Arc::new(build_mock_hub(&cfg)),
            mode: "mock".into(),
            limit_h2: cfg.gas_safety_limits.h2,
            limit_co: cfg.gas_safety_limits.co,
            flow_steps: custom_programs::flow_steps(&cfg),
            audit: Arc::new(AuditLog::new(dir.join("logs"))),
            storage: Arc::new(Mutex::new(Storage::open_in_memory().expect("db"))),
            auth: Arc::new(AuthService::new(900)),
            exp: Arc::new(ExpControl::new()),
            sim_speed: 180.0,
            exports_dir: dir.join("exports"),
            updater_enabled: false,
            telemetry_gen: Arc::new(AtomicU64::new(0)),
            cadence: Cadence::from_config(&cfg),
            idle_timeout_s: 900,
        }
    }

    /// Bootstrap an admin plus two ordinary users, each owning one experiment.
    /// Returns (admin_id, alice_id, bob_id); the session is left logged out.
    fn seed_users_and_experiments(state: &AppState) -> (String, String, String) {
        let now = epoch_now();
        let admin = state
            .auth
            .bootstrap(
                &lock(&state.storage),
                "admin",
                "管理员",
                "admin-password",
                now,
            )
            .expect("bootstrap");
        let mut ids = Vec::new();
        for (username, display) in [("alice", "爱丽丝"), ("bob", "鲍勃")] {
            let user = state
                .auth
                .create_user(
                    &lock(&state.storage),
                    username,
                    display,
                    "temp-password",
                    UserRole::User,
                    now,
                )
                .expect("create user");
            // Clear the forced first-login password change so these accounts can be used.
            lock(&state.storage)
                .set_user_password(
                    &user.user_id,
                    &user.password_hash,
                    false,
                    "2026-09-01T00:00:00",
                )
                .expect("clear must_change_password");
            ids.push(user.user_id);
        }
        state.auth.logout();
        let owners = [
            ("exp-admin", None),
            ("exp-alice", Some(ids[0].clone())),
            ("exp-bob", Some(ids[1].clone())),
        ];
        for (id, owner) in owners {
            lock(&state.storage)
                .insert_experiment(&ExperimentRow {
                    experiment_id: id.into(),
                    experiment_name: id.into(),
                    sample_id: Some(format!("{id}-sample")),
                    project_name: Some("project".into()),
                    experiment_time: None,
                    program_snapshot_json: None,
                    sample_name: "样品".into(),
                    sample_weight: 100.0,
                    start_time: "2026-09-01T08:00:00".into(),
                    end_time: None,
                    description: None,
                    operator: None,
                    experiment_type: Some("REDUCIBILITY".into()),
                    analysis_results_json: None,
                    created_at: "2026-09-01T08:00:00".into(),
                    owner_user_id: owner,
                })
                .expect("insert experiment");
        }
        (admin.user_id, ids[0].clone(), ids[1].clone())
    }

    fn login(state: &AppState, username: &str, password: &str) {
        state
            .auth
            .login(&lock(&state.storage), username, password, epoch_now())
            .expect("login");
    }

    // Paste inside src-tauri/src/lib.rs's existing `tests` module.
    // Reuses test_state, seed_users_and_experiments, login and params.
    // No production implementation or hardware I/O is introduced here.

    fn f06_native_draft(id: &str) -> Program {
        let mut program = standard_program("REDUCIBILITY").unwrap();
        program.id = id.into();
        program.name = "Native custom-program boundary test".into();
        program.source = ProgramSource::Custom;
        program.category = "custom".into();
        program.stages[2].target_temp = 850.0;
        program
    }

    #[test]
    fn f06_native_permissions_identity_conflicts_and_retained_history() {
        let state = test_state();
        let (admin_id, alice_id, _) = seed_users_and_experiments(&state);
        login(&state, "admin", "admin-password");

        // A structurally valid client provenance still cannot choose its audit actor.
        let forged = tmh_core::provenance::prepare_custom_revision(
            f06_native_draft("custom-native-access"),
            None,
            tmh_core::provenance::ProgramEditor {
                user_id: "forged-user".into(),
                display_name: "Forged operator".into(),
                edited_at: "1900-01-01T00:00:00Z".into(),
            },
            "Client-supplied reason",
        )
        .unwrap();
        let first =
            custom_programs::save(&state, forged, "Create shared research template").unwrap();
        let first_provenance = first.provenance.as_ref().unwrap();
        assert_eq!(first_provenance.revision, 1);
        assert_eq!(first_provenance.edited_by_user_id, admin_id);
        assert_eq!(first_provenance.edited_by, "管理员");
        assert_ne!(first_provenance.edited_at, "1900-01-01T00:00:00Z");
        assert_eq!(first_provenance.reason, "Create shared research template");

        let assert_mutations_denied = || {
            let mut update = first.clone();
            update.stages[2].target_temp = 825.0;
            assert!(custom_programs::save(&state, update, "Attempt restricted update").is_err());
            assert!(custom_programs::delete(&state, &first.id).is_err());
            let raw = lock(&state.storage)
                .get_custom_program(&first.id)
                .unwrap()
                .expect("a rejected mutation must leave the shared template intact");
            assert_eq!(serde_json::from_str::<Program>(&raw).unwrap(), first);
        };

        state.auth.logout();
        assert_mutations_denied();
        assert_eq!(
            custom_programs::history(&state, &first.id).unwrap_err(),
            "AUTH_REQUIRED"
        );

        login(&state, "alice", "temp-password");
        assert_mutations_denied();
        assert_eq!(
            custom_programs::history(&state, &first.id).unwrap(),
            vec![first.clone()]
        );
        state.auth.lock();
        assert_mutations_denied();
        assert_eq!(
            custom_programs::history(&state, &first.id).unwrap_err(),
            "AUTH_REQUIRED"
        );

        login(&state, "admin", "admin-password");
        state.exp.updating.store(true, Ordering::SeqCst);
        assert_mutations_denied();
        state.exp.updating.store(false, Ordering::SeqCst);
        state.exp.set_status(ExperimentStatus {
            running: true,
            phase: Phase::Running,
            experiment_id: Some("exp-alice".into()),
            owner_user_id: Some(alice_id),
            ..ExperimentStatus::idle()
        });
        assert_mutations_denied();
        state.exp.set_status(ExperimentStatus::idle());

        let mut update = first.clone();
        update.stages[2].target_temp = 800.0;
        update.provenance.as_mut().unwrap().edited_by_user_id = "forged-user".into();
        let second = custom_programs::save(&state, update, "Compare the next temperature").unwrap();
        assert_eq!(second.provenance.as_ref().unwrap().revision, 2);
        assert_eq!(
            second.provenance.as_ref().unwrap().edited_by_user_id,
            admin_id
        );

        // A stale editor must not replace revision 2 or append a spurious revision.
        let mut stale = first.clone();
        stale.stages[2].target_temp = 775.0;
        assert!(custom_programs::save(&state, stale, "Stale editor submission").is_err());
        let history = custom_programs::history(&state, &first.id).unwrap();
        assert_eq!(history.len(), 2);
        assert!(
            history.contains(&first),
            "the original revision was overwritten"
        );
        assert!(history.contains(&second), "the current revision was lost");

        custom_programs::delete(&state, &first.id).unwrap();
        assert!(lock(&state.storage)
            .get_custom_program(&first.id)
            .unwrap()
            .is_none());
        login(&state, "alice", "temp-password");
        assert_eq!(
            custom_programs::history(&state, &first.id).unwrap(),
            history,
            "deleting the current shared template must preserve readable revision evidence"
        );
    }

    #[test]
    fn f06_native_preflight_rejects_unversioned_templates_and_rechecks_changed_gas_limits() {
        let mut state = test_state();
        seed_users_and_experiments(&state);
        login(&state, "admin", "admin-password");

        // A structurally valid legacy template is readable, but has no provenance.
        let mut legacy = f06_native_draft("custom-native-legacy");
        legacy.provenance = None;
        lock(&state.storage)
            .save_custom_program(&legacy.id, &serde_json::to_string(&legacy).unwrap())
            .unwrap();
        let check = build_preflight(&state, &params(|p| p.experiment_type = legacy.id.clone()));
        assert!(
            check.program.is_some(),
            "legacy history must remain readable"
        );
        assert!(!check.ok);
        assert!(
            check
                .blocking
                .iter()
                .any(|issue| issue.code == "PROGRAM_INVALID"),
            "unready mock devices alone must not make this test pass"
        );

        // Save under the original valid limits, then lower one live limit. This
        // catches execution paths that trust a past successful save indefinitely.
        for (channel, id) in [(0, "custom-native-h2"), (3, "custom-native-co")] {
            let mut draft = f06_native_draft(id);
            draft.stages[2].gas[channel] = 1.0;
            let saved =
                custom_programs::save(&state, draft, "Prepare a changed-limit scenario").unwrap();
            let old_h2 = state.limit_h2;
            let old_co = state.limit_co;
            if channel == 0 {
                state.limit_h2 = 0.5;
            } else {
                state.limit_co = 0.5;
            }
            let check = build_preflight(&state, &params(|p| p.experiment_type = saved.id.clone()));
            assert!(!check.ok);
            assert!(
                check
                    .blocking
                    .iter()
                    .any(|issue| issue.code == "PROGRAM_GAS_INVALID"),
                "channel {channel}: excessive gas must block execution, not only warn or clamp"
            );
            assert!(
            !check
                .blocking
                .iter()
                .any(|issue| issue.code == "PROGRAM_INVALID"),
            "channel {channel}: the saved provenance is valid; only the new limit is incompatible"
        );
            state.limit_h2 = old_h2;
            state.limit_co = old_co;
        }
    }
    #[test]
    fn anonymous_session_is_refused_by_every_guard() {
        let state = test_state();
        seed_users_and_experiments(&state);
        assert_eq!(current_user(&state).unwrap_err(), "AUTH_REQUIRED");
        assert_eq!(current_admin(&state).unwrap_err(), "AUTH_REQUIRED");
        assert_eq!(
            require_experiment(&state, "exp-alice").unwrap_err(),
            "AUTH_REQUIRED"
        );
        assert_eq!(
            require_experiment_controller(&state).unwrap_err(),
            "AUTH_REQUIRED"
        );
    }

    #[test]
    fn a_user_cannot_reach_another_users_experiment() {
        let state = test_state();
        seed_users_and_experiments(&state);
        login(&state, "alice", "temp-password");
        assert!(require_experiment(&state, "exp-alice").is_ok());
        assert_eq!(
            require_experiment(&state, "exp-bob").unwrap_err(),
            "FORBIDDEN"
        );
        // Legacy rows with no owner stay admin-only after the v1 -> v2 migration.
        assert_eq!(
            require_experiment(&state, "exp-admin").unwrap_err(),
            "FORBIDDEN"
        );
        // A non-admin is refused admin-only commands (delete, user management, ownership).
        assert_eq!(current_admin(&state).unwrap_err(), "FORBIDDEN");
        // An id that does not exist must not leak as FORBIDDEN vs 不存在 either way round.
        assert!(require_experiment(&state, "exp-nope").is_err());
    }

    #[test]
    fn an_admin_reaches_every_experiment_including_unowned() {
        let state = test_state();
        seed_users_and_experiments(&state);
        login(&state, "admin", "admin-password");
        for id in ["exp-admin", "exp-alice", "exp-bob"] {
            assert!(require_experiment(&state, id).is_ok(), "admin denied {id}");
        }
        assert!(current_admin(&state).is_ok());
    }

    #[test]
    fn locking_the_session_revokes_access_until_the_next_login() {
        let state = test_state();
        seed_users_and_experiments(&state);
        login(&state, "alice", "temp-password");
        assert!(require_experiment(&state, "exp-alice").is_ok());
        state.auth.lock();
        assert_eq!(current_user(&state).unwrap_err(), "AUTH_REQUIRED");
        assert_eq!(
            require_experiment(&state, "exp-alice").unwrap_err(),
            "AUTH_REQUIRED"
        );
        login(&state, "alice", "temp-password");
        assert!(require_experiment(&state, "exp-alice").is_ok());
    }

    #[test]
    fn stage_actions_enforce_anonymous_owner_other_admin_locked_and_update_permissions() {
        let state = test_state();
        let (_, alice_id, _) = seed_users_and_experiments(&state);
        let status = ExperimentStatus {
            running: true,
            phase: Phase::Running,
            experiment_id: Some("exp-alice".into()),
            owner_user_id: Some(alice_id),
            stage_kind: Some(StageKind::Manual),
            stage_index: 3,
            ..ExperimentStatus::idle()
        };
        state.exp.set_status(status);
        let request = || {
            procedure::queue_action(
                &state,
                "exp-alice".into(),
                3,
                procedure::ActionKind::Confirm,
                "checked".into(),
            )
        };
        assert_eq!(request().unwrap_err(), "AUTH_REQUIRED");
        login(&state, "bob", "temp-password");
        assert_eq!(request().unwrap_err(), "FORBIDDEN");
        login(&state, "alice", "temp-password");
        assert!(request().is_ok());
        assert!(
            request().is_err(),
            "duplicate pending action must not replace evidence"
        );
        let first = lock(&state.exp.stage_action).take().unwrap();
        assert_eq!(first.operator, "爱丽丝");
        state.auth.lock();
        assert_eq!(request().unwrap_err(), "AUTH_REQUIRED");
        login(&state, "admin", "admin-password");
        assert!(request().is_ok());
        lock(&state.exp.stage_action).take();
        state.exp.updating.store(true, Ordering::SeqCst);
        assert!(request().is_err());
    }

    #[test]
    fn a_running_experiment_is_controlled_by_its_owner_or_an_admin_only() {
        let state = test_state();
        let (_admin_id, alice_id, _bob_id) = seed_users_and_experiments(&state);
        let mut running = ExperimentStatus::idle();
        running.running = true;
        running.experiment_id = Some("exp-alice".into());
        running.owner_user_id = Some(alice_id);
        state.exp.set_status(running);

        login(&state, "bob", "temp-password");
        assert_eq!(
            require_experiment_controller(&state).unwrap_err(),
            "FORBIDDEN",
            "another operator must not steer a run they do not own"
        );
        login(&state, "alice", "temp-password");
        assert!(require_experiment_controller(&state).is_ok());
        login(&state, "admin", "admin-password");
        assert!(
            require_experiment_controller(&state).is_ok(),
            "an admin can take over"
        );

        // With nothing running, any authenticated operator may drive the gas panel.
        state.exp.set_status(ExperimentStatus::idle());
        login(&state, "bob", "temp-password");
        assert!(require_experiment_controller(&state).is_ok());
    }

    #[test]
    fn a_temporary_password_blocks_everything_but_changing_it() {
        let state = test_state();
        seed_users_and_experiments(&state);
        // create_user leaves must_change_password set; make a third account that keeps it.
        login(&state, "admin", "admin-password");
        state
            .auth
            .create_user(
                &lock(&state.storage),
                "carol",
                "卡罗尔",
                "temp-password",
                UserRole::User,
                epoch_now(),
            )
            .expect("create carol");
        state.auth.logout();
        login(&state, "carol", "temp-password");
        assert_eq!(
            current_user(&state).unwrap_err(),
            "PASSWORD_CHANGE_REQUIRED"
        );
        assert_eq!(
            require_experiment_controller(&state).unwrap_err(),
            "PASSWORD_CHANGE_REQUIRED"
        );
    }

    #[test]
    fn a_deactivated_account_loses_its_live_session() {
        let state = test_state();
        let (_admin_id, alice_id, _bob) = seed_users_and_experiments(&state);
        login(&state, "alice", "temp-password");
        assert!(current_user(&state).is_ok());
        lock(&state.storage)
            .update_user(
                &alice_id,
                "爱丽丝",
                UserRole::User,
                false,
                "2026-09-01T09:00:00",
            )
            .expect("deactivate");
        assert_eq!(current_user(&state).unwrap_err(), "ACCOUNT_DISABLED");
    }

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
    fn unsupported_modes_cannot_label_mock_devices_as_real() {
        assert!(validate_runtime_mode("mock").is_ok());
        for mode in ["real", "replay", "", "MOCK", "typo"] {
            assert!(validate_runtime_mode(mode).is_err(), "{mode}");
        }
    }

    #[test]
    fn invalid_legacy_custom_duration_remains_readable_but_cannot_start() {
        let state = test_state();
        let mut p = standard_program("REDUCIBILITY").unwrap();
        p.id = "custom-legacy-duration".into();
        p.source = ProgramSource::Custom;
        p.stages[0].duration_min = 30.0;
        lock(&state.storage)
            .save_custom_program(&p.id, &serde_json::to_string(&p).unwrap())
            .unwrap();
        assert_eq!(all_programs(&lock(&state.storage)).unwrap().len(), 4);
        let preflight = build_preflight(&state, &params(|params| params.experiment_type = p.id));
        assert!(preflight
            .blocking
            .iter()
            .any(|e| e.code == "PROGRAM_INVALID"));
        assert!(!preflight.ok);
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
    fn reducing_target_matches_program() {
        assert_eq!(reducing_target(Some("REDUCIBILITY")), Some(900.0));
        assert_eq!(reducing_target(Some("LOW_TEMP_DEGRADATION")), Some(500.0));
        assert_eq!(reducing_target(Some("bogus")), None);
    }

    #[test]
    fn experiment_access_is_owner_or_admin_only() {
        assert!(may_access_experiment(
            UserRole::User,
            "alice",
            Some("alice")
        ));
        assert!(!may_access_experiment(UserRole::User, "alice", Some("bob")));
        assert!(!may_access_experiment(UserRole::User, "alice", None));
        assert!(may_access_experiment(UserRole::Admin, "admin", Some("bob")));
        assert!(may_access_experiment(UserRole::Admin, "admin", None));
    }

    #[test]
    fn running_status_hides_owner_metadata_from_other_users() {
        let mut raw = ExperimentStatus::idle();
        raw.running = true;
        raw.sample_name = Some("秘密样品".into());
        raw.owner_user_id = Some("alice".into());
        raw.owner_display_name = Some("Alice".into());

        let hidden = redact_experiment_status(raw.clone(), UserRole::User, "bob");
        assert!(hidden.occupied_by_other);
        assert!(!hidden.can_control);
        assert!(hidden.sample_name.is_none());
        assert!(hidden.owner_user_id.is_none());
        assert!(hidden.owner_display_name.is_none());

        let admin = redact_experiment_status(raw, UserRole::Admin, "admin");
        assert!(admin.can_control);
        assert_eq!(admin.sample_name.as_deref(), Some("秘密样品"));
    }

    fn row(status: &str, minutes: u64, weight: f64, offline: bool) -> SampleRow {
        SampleRow {
            record_json: None,
            experiment_id: "e1".into(),
            timestamp: "2026-09-01T08:00:00".into(),
            experiment_duration: Some(format!("{:02}:{:02}:00", minutes / 60, minutes % 60)),
            temperature: 900.0,
            weight,
            weight_loss: 100.0 - weight,
            co_flow: 4.5,
            co2_flow: 0.0,
            n2_flow: 10.5,
            h2_flow: 0.0,
            experiment_status: Some(status.into()),
            system_message: offline.then(|| "天平离线".to_string()),
        }
    }

    #[test]
    fn exact_reduction_endpoint_cannot_bridge_a_missing_measurement() {
        assert_eq!(value_at(&[(179.0, 40.0), (181.0, 42.0)], 180.0), Some(41.0));
        assert_eq!(
            value_at(&[(179.0, 40.0), (180.0, f64::NAN), (181.0, 42.0)], 180.0),
            None
        );
        let mut rows = vec![
            row("REDUCING", 1, 490.0, false),
            row("REDUCING", 3, 480.0, true),
        ];
        rows[0].record_json = Some("{\"stage_elapsed_s\":60}".into());
        rows[1].record_json = Some("{\"stage_elapsed_s\":180}".into());
        let curve = reduction_curve(
            &rows,
            500.0,
            500.0,
            &ReductionInput {
                w1: 0.0,
                w2: 60.0,
                manual_m1: None,
                manual_m180: None,
                manual_mt: None,
                manual_rate_points: None,
                source_record: None,
            },
            90.0,
        );
        assert_eq!(curve.len(), 3);
        assert!(curve[1].1.is_nan());
        assert!(curve[2].1.is_nan());
    }

    #[test]
    fn a_slow_sampling_interval_never_slows_the_safety_loop() {
        // The control loop evaluates the stop flag and the MFC/temperature offline abort,
        // so tying its period to an operator-editable sampling interval would let a
        // configured `SAMPLING.interval_s` of, say, 30 s delay cutting the gas by 30 s.
        assert_eq!(
            control_period(Duration::from_secs(30)),
            CONTROL_TICK_MAX,
            "a slow sampling cadence must not slow safety checks"
        );
        assert_eq!(control_period(Duration::from_secs(1)), CONTROL_TICK_MAX);
        // A faster cadence than the cap is honoured as configured.
        assert_eq!(
            control_period(Duration::from_millis(200)),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn tick_deadlines_do_not_drift_but_resync_after_a_stall() {
        // #43: a fixed period measured from the previous DEADLINE (not from "now") keeps a
        // 180 min hold at 180 min however long each tick's own work takes.
        let period = Duration::from_secs(1);
        let start = Instant::now();
        let mut deadline = start + period;
        // Each tick's work overruns by 300 ms; the schedule must not slip.
        for n in 2..=10 {
            let now = deadline + Duration::from_millis(300);
            deadline = next_deadline(deadline, period, now);
            assert_eq!(deadline, start + period * n, "tick {n} drifted");
        }
        // A stall longer than a whole period resyncs instead of firing a catch-up burst.
        let stalled = deadline + Duration::from_secs(30);
        assert_eq!(
            next_deadline(deadline, period, stalled),
            stalled + period,
            "a long stall resynchronises"
        );
    }

    #[test]
    fn the_sample_buffer_is_capped_and_counts_what_it_drops() {
        // #47: a run whose writes keep failing must not grow its buffer without bound.
        let mut buffer = SampleBuffer::default();
        for i in 0..(MAX_BUFFERED_SAMPLES + 25) {
            buffer.push(row("REDUCING", i as u64, 90.0, false));
        }
        assert_eq!(buffer.rows.len(), MAX_BUFFERED_SAMPLES);
        assert_eq!(buffer.dropped, 25);
        // The rows kept are the newest ones: the 25 oldest were evicted.
        let minutes = |r: &SampleRow| {
            r.experiment_duration
                .as_deref()
                .unwrap()
                .split(':')
                .take(2)
                .map(|v| v.parse::<usize>().unwrap())
                .fold(0, |sum, v| sum * 60 + v)
        };
        assert_eq!(minutes(buffer.rows.first().unwrap()), 25);
        assert_eq!(
            minutes(buffer.rows.last().unwrap()),
            MAX_BUFFERED_SAMPLES + 24
        );
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
    #[test]
    fn a_custom_reduction_uses_its_saved_endpoint_and_never_calls_it_r180() {
        let state = test_state();
        seed_users_and_experiments(&state);
        let mut row = lock(&state.storage)
            .list_experiments()
            .unwrap()
            .pop()
            .unwrap();
        row.sample_weight = 500.0;
        let mut program = standard_program("REDUCIBILITY").unwrap();
        program.source = ProgramSource::Custom;
        program.id = "custom-sixty".into();
        program.stages[2].duration_min = 60.0;
        row.program_snapshot_json = Some(serde_json::to_string(&program).unwrap());
        let input: ReductionInput=serde_json::from_value(serde_json::json!({"w1":10.0,"w2":60.0,"manual_m1":1500.0,"manual_mt":1450.0,"source_record":"60 min source"})).unwrap();
        let value = calculate_reduction(&row, &[], &serde_json::json!({}), &input).unwrap();
        assert_eq!(value["endpoint_min"], 60.0);
        assert!(value.get("ri").is_none());
        assert!((value["rt"].as_f64().unwrap() - 43.06201550387597).abs() < 1e-10);
        assert_eq!(value["standard_conformity"], "custom_conditions");
        let legacy: ReductionInput=serde_json::from_value(serde_json::json!({"w1":10.0,"w2":60.0,"manual_m1":1500.0,"manual_m180":1450.0,"source_record":"wrong endpoint"})).unwrap();
        assert!(calculate_reduction(&row, &[], &serde_json::json!({}), &legacy).is_err());
    }
    #[test]
    fn manual_missing_data_can_produce_a_traceable_standard_calculation() {
        let state = test_state();
        seed_users_and_experiments(&state);
        let mut row = lock(&state.storage)
            .list_experiments()
            .unwrap()
            .pop()
            .unwrap();
        row.sample_weight = 500.0;
        let mut input = ReductionInput {
            w1: 10.0,
            w2: 60.0,
            manual_m1: Some(1500.0),
            manual_m180: Some(1450.0),
            manual_mt: None,
            manual_rate_points: Some(vec![(0.0, 30.0), (10.0, 60.0)]),
            source_record: Some("原始记录 LAB-001".into()),
        };
        let value = calculate_reduction(&row, &[], &serde_json::json!({}), &input).unwrap();
        assert!((value["ri"].as_f64().unwrap() - 43.06201550387597).abs() < 1e-10);
        assert!((value["reduction_rate_index"].as_f64().unwrap() - 3.36).abs() < 1e-10);
        assert_eq!(value["manual_used"], true);
        assert_eq!(value["input"]["source_record"], "原始记录 LAB-001");
        input.source_record = None;
        assert!(calculate_reduction(&row, &[], &serde_json::json!({}), &input).is_err());
    }
}

#[cfg(test)]
mod persistence_tests;
