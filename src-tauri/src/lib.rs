//! Thin Tauri glue (docs/03). Owns the M2 `DeviceHub` in managed state, exposes it
//! to the webview through typed commands + a telemetry Channel, and hardens the shell
//! (single-instance). M3 runs the engine in manual-atmosphere mode only.

use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tmh_core::audit::{AuditEntry, AuditLog, AuditResult, Category};
use tmh_core::bus::{BalanceActor, DeviceHub, HubIntervals, MfcActor, TempActor};
use tmh_core::clamp::ClampResult;
use tmh_core::telemetry::{aggregate, TelemetrySnapshot};
use tmh_core::transport::MockTransport;

/// Managed application state.
struct AppState {
    hub: Arc<DeviceHub>,
    mode: String,
    limit_h2: f64,
    limit_co: f64,
    audit: AuditLog,
}

fn build_mock_hub() -> DeviceHub {
    let mut tm = MockTransport::temp();
    tm.set_temp_target_raw(9000); // demo furnace warming toward 900 °C
    let temp = TempActor::new(Box::new(tm), 0, 0.1, false);
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

// ---- App entrypoint ---------------------------------------------------------

/// Build and run the Tauri application.
pub fn run() {
    let mode = std::env::var("TMH_DEVICE_MODE").unwrap_or_else(|_| "mock".into());
    let audit_dir = std::env::temp_dir().join("tmh-lpf-900").join("logs");
    let _ = std::fs::create_dir_all(&audit_dir);
    let state = AppState {
        hub: Arc::new(build_mock_hub()),
        mode,
        limit_h2: 5.0,
        limit_co: 5.0,
        audit: AuditLog::new(audit_dir),
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
            subscribe_telemetry
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
