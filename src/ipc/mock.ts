// Browser-only synthetic engine (docs/08 dual-mode). Mirrors the Rust MockTransport
// so `pnpm dev:web` / the preview renders live data without a Tauri backend.

import type {
  ClampResult,
  ExperimentParams,
  ExperimentState,
  TareResult,
  TelemetrySnapshot,
} from "./types";

const H2_ABS = 10;
const CO_ABS = 10;
const limits = { h2: 5, co: 5 };
const sv = [0, 0, 0, 0]; // display L/min per gas
let pv = [0, 0, 0, 0];
let temps = new Array(9).fill(24).map((t, i) => t + i * 0.2); // slight per-channel spread
const target = 900;
let weight = 100;
let seq = 0;

const approach = (cur: number, tgt: number, frac: number) => cur + (tgt - cur) * frac;

export function tick(): TelemetrySnapshot {
  seq += 1;
  temps = temps.map((t) => approach(t, target, 0.015) + (Math.random() - 0.5) * 0.6);
  pv = pv.map((p, i) => {
    const s = sv[i] ?? 0;
    return approach(p, s, 0.18) + (s > 0 ? (Math.random() - 0.5) * 0.02 : 0);
  });
  return {
    seq,
    temps: temps.map((t) => Number(t.toFixed(1))),
    gas: pv.map((p, i) => {
      const s = sv[i] ?? 0;
      return { pv: Math.max(0, Number(p.toFixed(2))), sv: s, on: s > 0 };
    }),
    weight: Number((weight + (Math.random() - 0.5) * 0.002).toFixed(3)),
    health: { temp_connected: true, mfc_connected: true, balance_connected: true },
  };
}

export function mockSetFlow(gas: number, value: number): ClampResult {
  let applied = value;
  let clamped = false;
  let limit = Number.POSITIVE_INFINITY;
  if (gas === 0) limit = Math.min(limits.h2, H2_ABS);
  if (gas === 3) limit = Math.min(limits.co, CO_ABS);
  if (value > limit) {
    applied = limit;
    clamped = true;
  }
  sv[gas] = applied;
  return { applied, clamped, limit };
}

export function mockSetOnOff(gas: number, on: boolean): void {
  if (!on) sv[gas] = 0;
}

export function mockShutoff(): void {
  sv[0] = 0;
  sv[2] = 0;
  sv[3] = 0;
}

export function mockTare(): TareResult {
  weight = 0;
  return { success: true, a00_found: true, weight: 0 };
}

export function mockTelemetry(cb: (s: TelemetrySnapshot) => void): () => void {
  cb(tick());
  const id = setInterval(() => cb(tick()), 200);
  return () => clearInterval(id);
}

// ---- Experiment (browser mock) ----
let experiment: ExperimentState = { running: false };
const expListeners = new Set<(s: ExperimentState) => void>();
const notify = () => expListeners.forEach((f) => f(experiment));

export function mockStartExperiment(p: ExperimentParams): string {
  if (!p.sample_id.trim()) throw new Error("样品编号必填");
  if (!p.sample_name.trim()) throw new Error("样品名称必填");
  if (!(p.sample_weight > 0)) throw new Error("样品重量必须大于 0");
  if (!p.operator.trim()) throw new Error("操作员必填");
  if (experiment.running) throw new Error("实验已在运行");
  const id = `exp-${Date.now()}`;
  experiment = {
    running: true,
    experiment_id: id,
    mode: p.experiment_type,
    sample_name: p.sample_name,
    started_at: new Date().toISOString(),
  };
  notify();
  return id;
}

export function mockStopExperiment(): void {
  // Safety atmosphere: N2=5, flammables 0.
  sv[1] = 5;
  sv[0] = 0;
  sv[2] = 0;
  sv[3] = 0;
  experiment = { running: false };
  notify();
}

export function mockSubscribeExperiment(cb: (s: ExperimentState) => void): () => void {
  expListeners.add(cb);
  cb(experiment);
  return () => void expListeners.delete(cb);
}
