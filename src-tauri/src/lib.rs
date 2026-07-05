//! Thin Tauri glue (docs/03). Owns the M2 `DeviceHub` in managed state, exposes it
//! to the webview through typed commands + a telemetry Channel, and hardens the shell
//! (single-instance). M4 runs the real experiment engine: a per-run tick task drives
//! the furnace through its program stages, applies each stage's atmosphere through the
//! safety clamp, samples to SQLite, and guarantees the safety atmosphere on every exit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tmh_core::audit::{AuditEntry, AuditLog, AuditResult, Category};
use tmh_core::bus::{BalanceActor, DeviceHub, HubIntervals, MfcActor, TempActor};
use tmh_core::clamp::ClampResult;
use tmh_core::engine::{
    standard_program, ExperimentEngine, Phase, Program, Stage, StageKind, SAFETY_ATMOSPHERE,
};
use tmh_core::gb::{free_swelling, rdi, reducibility, reduction_rate_index};
use tmh_core::storage::{ExperimentRow, SampleRow, Storage};
use tmh_core::telemetry::{aggregate, TelemetrySnapshot};
use tmh_core::transport::MockTransport;

/// Registered experiment modes (custom modes land in a later slice).
const MODES: [&str; 3] = ["REDUCIBILITY", "LOW_TEMP_DEGRADATION", "FREE_SWELLING"];
/// Gas channel indices.
const GAS_H2: usize = 0;
const GAS_N2: usize = 1;
const GAS_CO2: usize = 2;
const GAS_CO: usize = 3;
/// Mock reduction removes this fraction of the sample mass (so GB analysis has signal).
const MOCK_REDUCTION_LOSS: f64 = 0.12;
/// Flush buffered samples to SQLite every N ticks.
const FLUSH_EVERY_TICKS: u32 = 5;

/// Managed application state.
struct AppState {
    hub: Arc<DeviceHub>,
    mode: String,
    limit_h2: f64,
    limit_co: f64,
    audit: Arc<AuditLog>,
    storage: Arc<Mutex<Storage>>,
    exp: Arc<ExpControl>,
    /// Simulated-seconds advanced per real second (mock time acceleration; 1.0 = real).
    sim_speed: f64,
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
        self.status.lock().unwrap().clone()
    }
    fn is_running(&self) -> bool {
        self.status.lock().unwrap().running
    }
    fn set_status(&self, s: ExperimentStatus) {
        *self.status.lock().unwrap() = s;
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
        }
    }
}

#[derive(serde::Deserialize)]
struct ExperimentParams {
    sample_id: String,
    sample_name: String,
    sample_weight: f64,
    operator: String,
    experiment_type: String,
    project_name: Option<String>,
    notes: Option<String>,
}

fn iso_now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Format an elapsed second count as HH:MM:SS.
fn fmt_hms(total_s: f64) -> String {
    let s = total_s.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
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

/// Simulated-time acceleration: `TMH_SIM_SPEED` if set, else fast for mock, real (1.0)
/// for hardware. Mock runs at 180× so a multi-hour program plays out in a few minutes.
fn sim_speed(mode: &str) -> f64 {
    std::env::var("TMH_SIM_SPEED")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(if mode == "mock" { 180.0 } else { 1.0 })
}

fn build_mock_hub() -> DeviceHub {
    // Idle furnace sits at ambient; a running experiment drives the target per stage.
    let temp = TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false);
    let mfc = MfcActor::new(
        Box::new(MockTransport::mfc()),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        5.0,
        5.0,
    );
    let balance = BalanceActor::new(Box::new(MockTransport::balance(100.0)));
    DeviceHub::spawn(temp, mfc, balance, HubIntervals::default())
}

// ---- Engine wiring helpers --------------------------------------------------

/// Apply a stage's atmosphere through the clamp and steer the mock furnace/balance.
/// The MFC bus actor clamps H2/CO at frame-build time — this is not an enforcement point.
fn apply_stage_atmosphere(hub: &DeviceHub, stage: &Stage, w0: f64) {
    for (gas, &v) in stage.gas.iter().enumerate() {
        drop(hub.request_set_flow(gas, v));
    }
    hub.set_temp_target_c(stage.target_temp);
    if stage.kind == StageKind::Reducing {
        // Mock only: reduction strips oxygen, so the sample loses mass over the stage.
        hub.set_balance_target_g(w0 * (1.0 - MOCK_REDUCTION_LOSS));
    }
}

/// The safety atmosphere (docs/05 §5.5): N2=5.0, H2=CO2=CO=0. Applied on EVERY exit.
fn apply_safety_atmosphere(hub: &DeviceHub) {
    for (gas, &v) in SAFETY_ATMOSPHERE.iter().enumerate() {
        drop(hub.request_set_flow(gas, v));
    }
    hub.set_temp_target_c(25.0);
}

/// Snapshot one sample row from live telemetry.
fn build_sample(hub: &DeviceHub, id: &str, w0: f64, total_s: f64, phase: Phase) -> SampleRow {
    let temperature = hub.temp_rx().borrow().temps[7].unwrap_or(0.0);
    let gas = hub.mfc_rx().borrow().gas;
    let weight = hub.balance_rx().borrow().weight.unwrap_or(w0);
    SampleRow {
        experiment_id: id.to_string(),
        timestamp: iso_now(),
        experiment_duration: Some(fmt_hms(total_s)),
        temperature,
        weight,
        weight_loss: (w0 - weight).max(0.0),
        co_flow: gas[GAS_CO].sv,
        co2_flow: gas[GAS_CO2].sv,
        n2_flow: gas[GAS_N2].sv,
        h2_flow: gas[GAS_H2].sv,
        experiment_status: Some(phase_str(phase).to_string()),
        system_message: None,
    }
}

/// Build the running-status snapshot from the engine's current position.
fn status_running(
    engine: &ExperimentEngine,
    id: &str,
    mode: &str,
    sample: &str,
    started: &str,
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
fn publish_running(
    app: &AppHandle,
    exp: &ExpControl,
    engine: &ExperimentEngine,
    id: &str,
    mode: &str,
    sample: &str,
    started: &str,
) {
    let status = status_running(engine, id, mode, sample, started);
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
    apply_safety_atmosphere(hub); // hard invariant — every exit path
    flush(storage, buffer);
    let end = iso_now();
    if let Ok(s) = storage.lock() {
        let _ = s.finish_experiment(id, &end);
    }
    audit.write(
        &AuditEntry::new(Category::Experiment, outcome, AuditResult::Success)
            .detail(serde_json::json!({ "id": id, "elapsed_s": total_s })),
    );
    let idle = ExperimentStatus::idle();
    exp.set_status(idle.clone());
    let _ = app.emit("experiment-state-changed", idle);
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
    sim_speed: f64,
) {
    std::thread::spawn(move || {
        let mut engine = ExperimentEngine::start(program);
        if let Some(s) = engine.current_stage() {
            apply_stage_atmosphere(&hub, s, w0);
        }
        publish_running(&app, &exp, &engine, &id, &mode, &sample, &started);

        let mut buffer: Vec<SampleRow> = Vec::new();
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
                    apply_stage_atmosphere(&hub, s, w0);
                    audit.write(
                        &AuditEntry::new(Category::Experiment, "stage_enter", AuditResult::Success)
                            .detail(serde_json::json!({
                                "id": id, "stage": engine.stage_index(), "name": s.name
                            })),
                    );
                }
            }

            buffer.push(build_sample(
                &hub,
                &id,
                w0,
                engine.total_elapsed_s(),
                engine.phase(),
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

            publish_running(&app, &exp, &engine, &id, &mode, &sample, &started);
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
    apply_safety_atmosphere(hub);
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

// ---- Commands ---------------------------------------------------------------

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
async fn set_gas_flow(app: AppHandle, gas: usize, value: f64) -> Result<ClampResult, String> {
    let hub = app.state::<AppState>().hub.clone();
    let clamp = hub
        .request_set_flow(gas, value)
        .await
        .map_err(|e| e.to_string())?;
    let st = app.state::<AppState>();
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
fn set_gas_on_off(state: State<AppState>, gas: usize, on: bool) {
    state.hub.set_on_off(gas, on);
    state.audit.write(
        &AuditEntry::new(Category::Gas, "on_off", AuditResult::Success)
            .detail(serde_json::json!({ "gas": gas, "on": on })),
    );
}

#[tauri::command]
fn open_all_valves(state: State<AppState>) {
    for g in 0..4 {
        state.hub.set_on_off(g, true);
    }
}

#[tauri::command]
fn close_all_valves(state: State<AppState>) {
    for g in 0..4 {
        state.hub.set_on_off(g, false);
    }
}

#[tauri::command]
fn shutoff_flammable_gases(state: State<AppState>) {
    state.hub.shutoff_flammable();
    state.audit.write(&AuditEntry::new(
        Category::Gas,
        "shutoff",
        AuditResult::Success,
    ));
}

#[tauri::command]
async fn tare_balance(app: AppHandle) -> Result<TareResultDto, String> {
    let hub = app.state::<AppState>().hub.clone();
    let r = hub.request_tare().await.map_err(|e| e.to_string())?;
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
fn subscribe_telemetry(app: AppHandle, channel: Channel<TelemetrySnapshot>) {
    let st = app.state::<AppState>();
    let temp_rx = st.hub.temp_rx();
    let mfc_rx = st.hub.mfc_rx();
    let bal_rx = st.hub.balance_rx();
    // The pump runs on its own thread; it exits when the frontend drops the channel.
    std::thread::spawn(move || {
        let mut seq = 0u64;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            seq += 1;
            let t = temp_rx.borrow().clone();
            let m = mfc_rx.borrow().clone();
            let b = bal_rx.borrow().clone();
            if channel.send(aggregate(seq, &t, &m, &b)).is_err() {
                break;
            }
        }
    });
}

// ---- Experiment -------------------------------------------------------------

#[tauri::command]
fn get_experiment_state(state: State<AppState>) -> ExperimentStatus {
    state.exp.snapshot()
}

#[tauri::command]
fn start_experiment(app: AppHandle, params: ExperimentParams) -> Result<String, String> {
    // Parameter validation (docs/05 §5.2).
    if params.sample_id.trim().is_empty() {
        return Err("样品编号必填".into());
    }
    if params.sample_name.trim().is_empty() {
        return Err("样品名称必填".into());
    }
    if params.sample_weight.is_nan() || params.sample_weight <= 0.0 {
        return Err("样品重量必须大于 0".into());
    }
    if params.operator.trim().is_empty() {
        return Err("操作员必填".into());
    }
    if !MODES.contains(&params.experiment_type.as_str()) {
        return Err("未知实验模式".into());
    }

    let st = app.state::<AppState>();
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
        operator: Some(params.operator.clone()),
        experiment_type: Some(params.experiment_type.clone()),
        analysis_results_json: None,
        created_at: now.clone(),
    };
    st.storage
        .lock()
        .unwrap()
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
    };
    st.exp.set_status(initial.clone());
    st.audit.write(
        &AuditEntry::new(Category::Experiment, "start", AuditResult::Success)
            .operator(params.operator.clone())
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
        st.sim_speed,
    );
    Ok(id)
}

#[tauri::command]
fn stop_experiment(app: AppHandle, reason: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    // Signal the tick task to finalize (it publishes idle within ≤1s).
    st.exp.stop.store(true, Ordering::Relaxed);
    // HARD INVARIANT (docs/05 §5.5): apply the safety atmosphere immediately here too —
    // belt-and-suspenders even if the tick task has already died.
    apply_safety_atmosphere(&st.hub);
    st.audit.write(
        &AuditEntry::new(Category::Experiment, "stop", AuditResult::Success)
            .detail(serde_json::json!({ "reason": reason })),
    );
    // If nothing was running, there is no task to publish idle — do it here.
    if !st.exp.is_running() {
        let idle = ExperimentStatus::idle();
        st.exp.set_status(idle.clone());
        let _ = app.emit("experiment-state-changed", idle);
    }
    Ok(())
}

#[tauri::command]
fn skip_stage(state: State<AppState>) -> Result<(), String> {
    if !state.exp.is_running() {
        return Err("没有正在运行的实验".into());
    }
    state.exp.skip.store(true, Ordering::Relaxed);
    Ok(())
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
    let rows = state
        .storage
        .lock()
        .unwrap()
        .list_experiments()
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(summary_of).collect())
}

#[tauri::command]
fn get_experiment_detail(state: State<AppState>, id: String) -> Result<ExperimentDetail, String> {
    let s = state.storage.lock().unwrap();
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
    let rows = state
        .storage
        .lock()
        .unwrap()
        .get_samples(&id, max_points.unwrap_or(2000))
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(sample_dto).collect())
}

#[tauri::command]
fn delete_experiment(state: State<AppState>, id: String) -> Result<(), String> {
    // Never delete the run currently in progress.
    if state.exp.snapshot().experiment_id.as_deref() == Some(id.as_str()) {
        return Err("无法删除正在运行的实验".into());
    }
    state
        .storage
        .lock()
        .unwrap()
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
    state
        .storage
        .lock()
        .unwrap()
        .set_analysis_results(id, &json)
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(Category::Experiment, "analyze", AuditResult::Success)
            .detail(serde_json::json!({ "id": id, "standard": standard })),
    );
    Ok(())
}

fn require_experiment(state: &AppState, id: &str) -> Result<(), String> {
    let exists = state
        .storage
        .lock()
        .unwrap()
        .get_experiment(id)
        .map_err(|e| e.to_string())?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err("实验不存在".into())
    }
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
        let s = state.storage.lock().unwrap();
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

// ---- App entrypoint ---------------------------------------------------------

/// Build and run the Tauri application.
pub fn run() {
    let mode = std::env::var("TMH_DEVICE_MODE").unwrap_or_else(|_| "mock".into());
    let sim_speed = sim_speed(&mode);
    let data_root = std::env::temp_dir().join("tmh-lpf-900");
    let audit_dir = data_root.join("logs");
    let db_dir = data_root.join("data");
    let _ = std::fs::create_dir_all(&audit_dir);
    let _ = std::fs::create_dir_all(&db_dir);
    let storage = Storage::open(&db_dir.join("experiments.db")).expect("open experiments db");
    let hub = Arc::new(build_mock_hub());
    let audit = Arc::new(AuditLog::new(audit_dir));

    // Crash recovery before the UI comes up (safety atmosphere for any open run).
    recover_unfinished(&hub, &storage, &audit);

    let state = AppState {
        hub,
        mode,
        limit_h2: 5.0,
        limit_co: 5.0,
        audit,
        storage: Arc::new(Mutex::new(storage)),
        exp: Arc::new(ExpControl::new()),
        sim_speed,
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
        .manage(state)
        .invoke_handler(tauri::generate_handler![
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
            get_experiment_state,
            start_experiment,
            stop_experiment,
            skip_stage,
            list_experiments,
            get_experiment_detail,
            get_experiment_samples,
            delete_experiment,
            analyze_gb13241,
            analyze_gb13242,
            analyze_gb13240
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
