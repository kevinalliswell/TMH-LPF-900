// Browser-only synthetic engine (docs/08 dual-mode). Mirrors the Rust backend closely
// enough that `pnpm dev:web` / the preview renders a faithful M4 demo without Tauri:
// a target-driven furnace/balance, the experiment stage engine (heat → hold → reduce →
// cool → complete + safety), an in-memory experiment history, and the GB analyses.

import type {
  ClampResult,
  ExperimentDetail,
  ExperimentParams,
  ExperimentPreflightResult,
  ExperimentState,
  ExperimentSummary,
  ProgramDto,
  SampleDto,
  StageDto,
  StageKind,
  TareResult,
  TelemetrySnapshot,
} from "./types";

// ---- shared device state (targets drive the physics; tick() reports) ----
const H2_ABS = 10;
const CO_ABS = 10;
const limits = { h2: 5, co: 5 };
const sv = [0, 0, 0, 0]; // display L/min per gas [H2, N2, CO2, CO]
let pv = [0, 0, 0, 0];
let temps = new Array(9).fill(24).map((t, i) => t + i * 0.2);
let furnaceTarget = 25; // °C the furnace eases toward (idle ambient)
let weight = 100;
let weightTarget = 100;
let seq = 0;

const approach = (cur: number, tgt: number, frac: number) => cur + (tgt - cur) * frac;

function clampGas(gas: number, value: number): number {
  if (!Number.isFinite(value) || value < 0) return 0; // mirror the backend clamp (#4)
  let limit = Number.POSITIVE_INFINITY;
  if (gas === 0) limit = Math.min(limits.h2, H2_ABS);
  if (gas === 3) limit = Math.min(limits.co, CO_ABS);
  return Math.min(value, limit);
}

export function tick(): TelemetrySnapshot {
  seq += 1;
  temps = temps.map((t) => approach(t, furnaceTarget, 0.08) + (Math.random() - 0.5) * 0.6);
  weight = approach(weight, weightTarget, 0.08);
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
    weight: Number(weight.toFixed(3)),
    health: { temp_connected: true, mfc_connected: true, balance_connected: true },
  };
}

export function mockSetFlow(gas: number, value: number): ClampResult {
  const applied = clampGas(gas, value);
  const limit = gas === 0 ? Math.min(limits.h2, H2_ABS) : gas === 3 ? Math.min(limits.co, CO_ABS) : Infinity;
  sv[gas] = applied;
  return { applied, clamped: applied < value, limit };
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
  weightTarget = 0;
  return { success: true, a00_found: true, weight: 0 };
}

export function mockTelemetry(cb: (s: TelemetrySnapshot) => void): () => void {
  cb(tick());
  const id = setInterval(() => cb(tick()), 200);
  return () => clearInterval(id);
}

// ---- program tables (mirror tmh_core::engine::standard_program) ----
function st(
  name: string,
  kind: StageKind,
  gas: number[],
  target_temp: number,
  tolerance: number,
  duration_min: number,
  ramp_rate: number,
  description: string,
): StageDto {
  return { name, kind, gas, target_temp, tolerance, duration_min, ramp_rate, description };
}

const PROGRAMS: ProgramDto[] = [
  {
    id: "REDUCIBILITY",
    name: "GB/T 13241 还原性",
    source: "standard",
    stages: [
      st("升温", "HEATING", [0, 5, 0, 0], 900, 5, 0, 10, "升温至900℃,N₂保护"),
      st("恒温", "STABILIZING", [0, 15, 0, 0], 900, 5, 30, 0, "900℃恒温30min,N₂气氛"),
      st("还原", "REDUCING", [0, 10.5, 0, 4.5], 900, 5, 180, 0, "还原180min,30%CO+70%N₂"),
      st("冷却", "COOLING", [0, 5, 0, 0], 25, 0, 0, -10, "N₂保护冷却至室温"),
    ],
  },
  {
    id: "LOW_TEMP_DEGRADATION",
    name: "GB/T 13242 低温粉化",
    source: "standard",
    stages: [
      st("升温", "HEATING", [0, 5, 0, 0], 500, 5, 0, 10, "升温至500℃,N₂保护"),
      st("恒温", "STABILIZING", [0, 15, 0, 0], 500, 5, 30, 0, "500℃恒温30min,N₂气氛"),
      st("还原", "REDUCING", [0, 9, 3, 3], 500, 5, 60, 0, "还原60min,20%CO+20%CO₂+60%N₂"),
      st("冷却", "COOLING", [0, 5, 0, 0], 25, 0, 0, -10, "N₂保护冷却至室温"),
    ],
  },
  {
    id: "FREE_SWELLING",
    name: "GB/T 13240 自由膨胀",
    source: "standard",
    stages: [
      st("升温", "HEATING", [0, 10, 0, 0], 900, 5, 0, 10, "升温至900℃,N₂保护"),
      st("恒温", "STABILIZING", [0, 15, 0, 0], 900, 5, 15, 0, "900℃恒温15min,N₂气氛"),
      st("还原", "REDUCING", [0, 10.5, 0, 4.5], 900, 5, 60, 0, "还原60min,30%CO+70%N₂"),
      st("冷却", "COOLING", [0, 5, 0, 0], 50, 0, 0, -10, "N₂保护冷却至50℃"),
    ],
  },
];

let customPrograms: ProgramDto[] = [];

function cloneProgram(program: ProgramDto): ProgramDto {
  return {
    ...program,
    stages: program.stages.map((s) => ({ ...s, gas: [...s.gas] })),
  };
}

function allPrograms(): ProgramDto[] {
  return [...PROGRAMS, ...customPrograms].map(cloneProgram);
}

export function mockListPrograms(): ProgramDto[] {
  return allPrograms();
}

function validateProgram(program: ProgramDto): string | null {
  if (!program.name.trim()) return "模式名称必填";
  if (program.stages.length === 0) return "至少需要一个阶段";
  for (const [i, stage] of program.stages.entries()) {
    const n = i + 1;
    if (!stage.name.trim()) return `第 ${n} 阶段名称必填`;
    if (!Number.isFinite(stage.target_temp)) return `第 ${n} 阶段目标温度无效`;
    if (!Number.isFinite(stage.tolerance) || stage.tolerance < 0) return `第 ${n} 阶段容差必须为非负数`;
    if (!Number.isFinite(stage.duration_min) || stage.duration_min < 0) return `第 ${n} 阶段时长必须为非负数`;
    if (!Number.isFinite(stage.ramp_rate)) return `第 ${n} 阶段升温率无效`;
    if (stage.gas.some((v) => !Number.isFinite(v) || v < 0)) return `第 ${n} 阶段气体流量必须为非负数`;
    if (stage.gas.reduce((a, b) => a + b, 0) <= 0) return `第 ${n} 阶段总流量必须大于 0`;
  }
  return null;
}

function flammableWarnings(program: ProgramDto): ExperimentPreflightResult["warnings"] {
  const warnings: ExperimentPreflightResult["warnings"] = [];
  program.stages.forEach((stage, i) => {
    const h2 = stage.gas[0] ?? 0;
    const co = stage.gas[3] ?? 0;
    if (h2 > limits.h2) {
      warnings.push({
        code: "H2_LIMIT",
        message_zh: `第 ${i + 1} 阶段 H₂ 设定 ${h2.toFixed(1)} L/min 超过安全上限 ${limits.h2.toFixed(1)} L/min，执行时会被钳制`,
      });
    }
    if (co > limits.co) {
      warnings.push({
        code: "CO_LIMIT",
        message_zh: `第 ${i + 1} 阶段 CO 设定 ${co.toFixed(1)} L/min 超过安全上限 ${limits.co.toFixed(1)} L/min，执行时会被钳制`,
      });
    }
  });
  return warnings;
}

export function mockSaveCustomProgram(program: ProgramDto): ProgramDto {
  const saved = cloneProgram(program);
  saved.id = saved.id.trim() || `custom-${Date.now()}`;
  saved.name = saved.name.trim();
  saved.source = "custom";
  if (PROGRAMS.some((p) => p.id === saved.id)) throw new Error("标准模式不可覆盖，请复制为自定义模式");
  const error = validateProgram(saved);
  if (error) throw new Error(error);
  const idx = customPrograms.findIndex((p) => p.id === saved.id);
  if (idx >= 0) customPrograms[idx] = cloneProgram(saved);
  else customPrograms.push(cloneProgram(saved));
  return cloneProgram(saved);
}

export function mockDeleteCustomProgram(id: string): void {
  if (PROGRAMS.some((p) => p.id === id)) throw new Error("标准模式不可删除");
  const before = customPrograms.length;
  customPrograms = customPrograms.filter((p) => p.id !== id);
  if (customPrograms.length === before) throw new Error("自定义模式不存在");
}

export function mockPreflightExperiment(p: ExperimentParams): ExperimentPreflightResult {
  const blocking: ExperimentPreflightResult["blocking"] = [];
  if (!p.sample_id.trim()) blocking.push({ code: "SAMPLE_ID", message_zh: "样品编号必填" });
  if (!p.sample_name.trim()) blocking.push({ code: "SAMPLE_NAME", message_zh: "样品名称必填" });
  if (!(p.sample_weight > 0)) blocking.push({ code: "SAMPLE_WEIGHT", message_zh: "样品重量必须大于 0" });
  if (!p.operator.trim()) blocking.push({ code: "OPERATOR", message_zh: "操作员必填" });
  if (experiment.running) blocking.push({ code: "EXPERIMENT_RUNNING", message_zh: "实验已在运行" });
  const program = allPrograms().find((pr) => pr.id === p.experiment_type) ?? null;
  if (!program) blocking.push({ code: "PROGRAM_NOT_FOUND", message_zh: "未知实验模式" });
  const warnings = program ? flammableWarnings(program) : [];
  if (program && p.sample_weight > 0) {
    const diff = Math.abs(weight - p.sample_weight);
    const threshold = Math.max(p.sample_weight * 0.02, 0.1);
    if (diff > threshold) {
      warnings.push({
        code: "WEIGHT_MISMATCH",
        message_zh: `当前天平读数 ${weight.toFixed(3)} g 与输入样品重量 ${p.sample_weight.toFixed(3)} g 差异较大，请确认初始重量`,
      });
    }
  }
  return { ok: blocking.length === 0, blocking, warnings, program };
}

// ---- experiment stage engine (browser) ----
const MOCK_LOSS = 0.12;
const SIM_DT = 300; // simulated seconds advanced per mock tick
const EXP_TICK_MS = 400;

function fmtHms(totalS: number): string {
  const s = Math.max(0, Math.floor(totalS));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}

function stageReached(s: StageDto, t8: number, elapsedS: number): boolean {
  const atTemp = Math.abs(t8 - s.target_temp) <= s.tolerance;
  if (s.duration_min > 0) return atTemp && elapsedS >= s.duration_min * 60;
  if (s.kind === "COOLING") return t8 <= s.target_temp + 5;
  return atTemp;
}

function applyStage(s: StageDto, w0: number): void {
  s.gas.forEach((v, i) => (sv[i] = clampGas(i, v)));
  furnaceTarget = s.target_temp;
  if (s.kind === "REDUCING") weightTarget = w0 * (1 - MOCK_LOSS);
}

interface MockRun {
  program: ProgramDto;
  id: string;
  mode: string;
  sample: string;
  operator: string;
  notes: string;
  started: string;
  w0: number;
  stageIndex: number;
  stageElapsedS: number;
  totalElapsedS: number;
  samples: SampleDto[];
  timer: ReturnType<typeof setInterval> | null;
  skip: boolean;
}

let run: MockRun | null = null;
let experiment: ExperimentState = { running: false };
const expListeners = new Set<(s: ExperimentState) => void>();
const notify = () => expListeners.forEach((f) => f(experiment));

function runningState(r: MockRun): ExperimentState {
  const s = r.program.stages[r.stageIndex];
  return {
    running: true,
    experiment_id: r.id,
    mode: r.mode,
    sample_name: r.sample,
    started_at: r.started,
    phase: "RUNNING",
    stage_index: r.stageIndex,
    total_stages: r.program.stages.length,
    stage_name: s?.name ?? null,
    stage_kind: s?.kind ?? null,
    target_temp: s?.target_temp ?? 0,
    tolerance: s?.tolerance ?? 0,
    stage_elapsed_s: r.stageElapsedS,
    total_elapsed_s: r.totalElapsedS,
  };
}

function sampleNow(r: MockRun, t8: number): SampleDto {
  return {
    timestamp: new Date().toISOString(),
    experiment_duration: fmtHms(r.totalElapsedS),
    temperature: Number(t8.toFixed(1)),
    weight: Number(weight.toFixed(3)),
    weight_loss: Number(Math.max(0, r.w0 - weight).toFixed(3)),
    co_flow: sv[3] ?? 0,
    co2_flow: sv[2] ?? 0,
    n2_flow: sv[1] ?? 0,
    h2_flow: sv[0] ?? 0,
    experiment_status: "RUNNING",
  };
}

function archive(r: MockRun): void {
  const detail: ExperimentDetail = {
    experiment_id: r.id,
    experiment_name: r.sample,
    sample_name: r.sample,
    sample_weight: r.w0,
    start_time: r.started,
    end_time: new Date().toISOString(),
    description: r.notes || null,
    operator: r.operator || null,
    experiment_type: r.mode,
    analysis: null,
    program_snapshot: cloneProgram(r.program),
    created_at: r.started,
    sample_count: r.samples.length,
  };
  history.set(r.id, { detail, samples: r.samples });
}

function stopRun(): void {
  if (run?.timer) clearInterval(run.timer);
  run = null;
}

function expTick(): void {
  if (!run) return;
  const stage = run.program.stages[run.stageIndex];
  if (!stage) {
    completeRun();
    return;
  }
  run.stageElapsedS += SIM_DT;
  run.totalElapsedS += SIM_DT;
  const t8 = temps[7] ?? furnaceTarget;
  run.samples.push(sampleNow(run, t8));

  if (run.skip || stageReached(stage, t8, run.stageElapsedS)) {
    run.skip = false;
    run.stageIndex += 1;
    run.stageElapsedS = 0;
    const next = run.program.stages[run.stageIndex];
    if (!next) {
      completeRun();
      return;
    }
    applyStage(next, run.w0);
  }
  experiment = runningState(run);
  notify();
}

function completeRun(): void {
  if (!run) return;
  // Safety atmosphere on completion.
  sv[0] = 0;
  sv[1] = 5;
  sv[2] = 0;
  sv[3] = 0;
  furnaceTarget = 25;
  weightTarget = weight;
  archive(run);
  stopRun();
  experiment = { running: false };
  notify();
}

export function mockStartExperiment(p: ExperimentParams): string {
  const preflight = mockPreflightExperiment(p);
  if (preflight.blocking[0]) throw new Error(preflight.blocking[0].message_zh);
  const program = preflight.program;
  if (!program) throw new Error("未知实验模式");

  const id = `exp-${Date.now()}`;
  const started = new Date().toISOString();
  run = {
    program: cloneProgram(program),
    id,
    mode: p.experiment_type,
    sample: p.sample_name,
    operator: p.operator,
    notes: p.notes ?? "",
    started,
    w0: p.sample_weight,
    stageIndex: 0,
    stageElapsedS: 0,
    totalElapsedS: 0,
    samples: [],
    timer: null,
    skip: false,
  };
  weight = p.sample_weight;
  weightTarget = p.sample_weight;
  const first = run.program.stages[0];
  if (first) applyStage(first, run.w0);
  experiment = runningState(run);
  notify();
  run.timer = setInterval(expTick, EXP_TICK_MS);
  return id;
}

export function mockStopExperiment(): void {
  // Safety atmosphere: N2=5, flammables 0.
  sv[0] = 0;
  sv[1] = 5;
  sv[2] = 0;
  sv[3] = 0;
  furnaceTarget = 25;
  if (run) {
    archive(run); // stopped runs still land in history (end_time stamped)
    stopRun();
  }
  experiment = { running: false };
  notify();
}

export function mockSkipStage(reason: string): void {
  if (!reason.trim()) throw new Error("跳过阶段原因必填");
  if (run) run.skip = true;
}

export function mockSubscribeExperiment(cb: (s: ExperimentState) => void): () => void {
  expListeners.add(cb);
  cb(experiment);
  return () => void expListeners.delete(cb);
}

// ---- in-memory history ----
interface HistItem {
  detail: ExperimentDetail;
  samples: SampleDto[];
}
const history = new Map<string, HistItem>();

/** Synchronously simulate a full run to fabricate a seed history entry. */
function simulate(program: ProgramDto, w0: number): SampleDto[] {
  let t = 25;
  let wt = w0;
  let stageIndex = 0;
  let stageEl = 0;
  let totalEl = 0;
  const samples: SampleDto[] = [];
  let guard = 0;
  while (stageIndex < program.stages.length && guard++ < 5000) {
    const s = program.stages[stageIndex];
    if (!s) break;
    t += (s.target_temp - t) * 0.1;
    if (s.kind === "REDUCING") wt += (w0 * (1 - MOCK_LOSS) - wt) * 0.06;
    stageEl += 60;
    totalEl += 60;
    samples.push({
      timestamp: new Date(Date.parse("2026-06-01T09:00:00") + totalEl * 1000).toISOString(),
      experiment_duration: fmtHms(totalEl),
      temperature: Number(t.toFixed(1)),
      weight: Number(wt.toFixed(3)),
      weight_loss: Number(Math.max(0, w0 - wt).toFixed(3)),
      co_flow: s.gas[3] ?? 0,
      co2_flow: s.gas[2] ?? 0,
      n2_flow: s.gas[1] ?? 0,
      h2_flow: s.gas[0] ?? 0,
      experiment_status: "RUNNING",
    });
    if (stageReached(s, t, stageEl)) {
      stageIndex += 1;
      stageEl = 0;
    }
  }
  return samples;
}

function seed(id: string, name: string, mode: string, start: string, w0: number): void {
  const program = PROGRAMS.find((p) => p.id === mode);
  if (!program) return;
  const samples = simulate(program, w0);
  history.set(id, {
    detail: {
      experiment_id: id,
      experiment_name: name,
      sample_name: name,
      sample_weight: w0,
      start_time: start,
      end_time: new Date(Date.parse(start) + samples.length * 60000).toISOString(),
      description: "演示数据",
      operator: "张工",
      experiment_type: mode,
      analysis: null,
      program_snapshot: cloneProgram(program),
      created_at: start,
      sample_count: samples.length,
    },
    samples,
  });
}
seed("exp-demo-2", "鞍钢球团-B", "REDUCIBILITY", "2026-06-02T09:00:00", 100);
seed("exp-demo-1", "首钢烧结矿-A", "LOW_TEMP_DEGRADATION", "2026-06-01T14:30:00", 500);

export function mockListExperiments(): ExperimentSummary[] {
  return [...history.values()]
    .map((h) => ({
      experiment_id: h.detail.experiment_id,
      experiment_name: h.detail.experiment_name,
      sample_name: h.detail.sample_name,
      sample_weight: h.detail.sample_weight,
      start_time: h.detail.start_time,
      end_time: h.detail.end_time,
      operator: h.detail.operator,
      experiment_type: h.detail.experiment_type,
      has_analysis: h.detail.analysis != null,
    }))
    .sort((a, b) => b.start_time.localeCompare(a.start_time));
}

export function mockGetExperimentDetail(id: string): ExperimentDetail {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  return { ...h.detail, sample_count: h.samples.length };
}

export function mockGetExperimentSamples(id: string, maxPoints?: number): SampleDto[] {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  const all = h.samples;
  const cap = maxPoints ?? 2000;
  if (cap <= 0 || all.length <= cap) return all.map((s) => ({ ...s }));
  const stride = Math.ceil(all.length / cap);
  return all.filter((_, i) => i % stride === 0).map((s) => ({ ...s }));
}

export function mockDeleteExperiment(id: string): void {
  history.delete(id);
}

// ---- export / report (browser fallbacks; Tauri writes files in Rust) ----
const CSV_COLUMNS = [
  "时间",
  "实验时长",
  "温度(℃)",
  "质量(g)",
  "失重(g)",
  "CO(L/min)",
  "CO2(L/min)",
  "N2(L/min)",
  "H2(L/min)",
  "状态",
];

export function mockExportText(id: string, sep: string): string {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  const rows = h.samples.map((s) =>
    [
      s.timestamp,
      s.experiment_duration ?? "",
      s.temperature.toFixed(1),
      s.weight.toFixed(3),
      s.weight_loss.toFixed(3),
      s.co_flow.toFixed(2),
      s.co2_flow.toFixed(2),
      s.n2_flow.toFixed(2),
      s.h2_flow.toFixed(2),
      s.experiment_status ?? "",
    ].join(sep),
  );
  return "﻿" + [CSV_COLUMNS.join(sep), ...rows].join("\r\n") + "\r\n";
}

export function mockReportHtml(id: string): string {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  const d = h.detail;
  const a = (d.analysis ?? null) as Record<string, unknown> | null;
  const meta = [
    ["实验编号", d.experiment_id],
    ["样品名称", d.sample_name],
    ["实验类型", d.experiment_type ?? "—"],
    ["操作员", d.operator ?? "—"],
    ["样品重量", `${d.sample_weight} g`],
    ["开始时间", d.start_time],
    ["结束时间", d.end_time ?? "—"],
    ["采样点数", String(h.samples.length)],
  ]
    .map(([k, v]) => `<tr><th>${k}</th><td>${v}</td></tr>`)
    .join("");
  const analysis = a
    ? `<pre>${JSON.stringify(a, null, 2)}</pre>`
    : '<p style="color:#999">尚未进行结果分析。</p>';
  return `<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><title>检测报告 ${d.experiment_id}</title>\
<style>body{font-family:'Microsoft YaHei',system-ui,sans-serif;max-width:820px;margin:24px auto;padding:0 16px;color:#1a1a1a}\
h1{font-size:20px}h2{font-size:14px;border-bottom:2px solid #d9631f;padding-bottom:4px;margin-top:24px}\
table{border-collapse:collapse}th,td{border:1px solid #e2e2e2;padding:5px 10px;text-align:left}th{background:#f6f6f6}\
pre{background:#f6f6f6;padding:10px;border-radius:4px;font-size:12px}</style></head><body>\
<h1>铁矿石冶金性能检测报告</h1><div style="color:#666;font-size:12px">TMH-LPF-900 · 北京科技大学（浏览器预览）</div>\
<h2>实验信息</h2><table>${meta}</table><h2>结果分析</h2>${analysis}</body></html>`;
}

// ---- GB analyses (mirror tmh_core::gb) ----
function parseHmsMin(s?: string | null): number | null {
  if (!s) return null;
  const parts = s.split(":");
  if (parts.length < 3) return null;
  const h = Number(parts[0]);
  const m = Number(parts[1]);
  const sec = Number(parts[2]);
  if ([h, m, sec].some(Number.isNaN)) return null;
  return h * 60 + m + sec / 60;
}

function slope(pts: [number, number][]): number | null {
  if (pts.length < 2) return null;
  const n = pts.length;
  const sx = pts.reduce((a, p) => a + p[0], 0);
  const sy = pts.reduce((a, p) => a + p[1], 0);
  const sxx = pts.reduce((a, p) => a + p[0] * p[0], 0);
  const sxy = pts.reduce((a, p) => a + p[0] * p[1], 0);
  const d = n * sxx - sx * sx;
  return d === 0 ? null : (n * sxy - sx * sy) / d;
}

export function mockAnalyze13241(id: string, o2Pct: number): Record<string, unknown> {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  const w0 = h.detail.sample_weight;
  const last = h.samples[h.samples.length - 1];
  const wt = last ? last.weight : w0;
  if (w0 <= 0 || o2Pct <= 0) throw new Error("W0 或 O2% 无效");
  const deltaW = w0 - wt;
  const pts = h.samples
    .map((s): [number, number] | null => {
      const m = parseHmsMin(s.experiment_duration);
      return m == null ? null : [m, ((w0 - s.weight) / (w0 * (o2Pct / 100))) * 100];
    })
    .filter((p): p is [number, number] => p !== null);
  const value = {
    standard: "GB/T 13241",
    w0,
    wt,
    o2_pct: o2Pct,
    rt: (deltaW / (w0 * (o2Pct / 100))) * 100,
    delta_w: deltaW,
    reduction_rate_index: slope(pts),
    computed_at: new Date().toISOString(),
  };
  h.detail.analysis = value;
  return value;
}

export function mockAnalyze13242(
  id: string,
  m: { m0: number; mGt63: number; m31563: number; m05315: number; mLt05: number },
): Record<string, unknown> {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  if (m.m0 <= 0) throw new Error("m0 无效");
  const total = m.mGt63 + m.m31563 + m.m05315 + m.mLt05;
  const value = {
    standard: "GB/T 13242",
    m0: m.m0,
    m_gt63: m.mGt63,
    m_315_63: m.m31563,
    m_05_315: m.m05315,
    m_lt05: m.mLt05,
    rdi_plus_6_3: (m.mGt63 / m.m0) * 100,
    rdi_plus_3_15: ((m.mGt63 + m.m31563) / m.m0) * 100,
    rdi_minus_0_5: (m.mLt05 / m.m0) * 100,
    sieve_warning: Math.abs(total - m.m0) / m.m0 > 0.02,
    computed_at: new Date().toISOString(),
  };
  h.detail.analysis = value;
  return value;
}

export function mockAnalyze13240(id: string, volumes: [number, number][]): Record<string, unknown> {
  const h = history.get(id);
  if (!h) throw new Error("实验不存在");
  const per = volumes.filter(([v0]) => v0 > 0).map(([v0, v1]) => ((v1 - v0) / v0) * 100);
  if (!per.length) throw new Error("无有效试样体积");
  const value = {
    standard: "GB/T 13240",
    per_pellet: per,
    average: per.reduce((a, b) => a + b, 0) / per.length,
    computed_at: new Date().toISOString(),
  };
  h.detail.analysis = value;
  return value;
}
