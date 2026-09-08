import { programSnapshotHtml } from "./reportPreview";
import {
  programValueErrors,
  customShapeError,
  gasConditionErrors,
  executableProgramError,
  standardParent,
  conditionChanges,
} from "./programRules";
import standardPrograms from "../../config/standard_programs.json";
// Browser-only synthetic engine (docs/08 dual-mode). Mirrors the Rust backend closely
// enough that `pnpm dev:web` / the preview renders a faithful M4 demo without Tauri:
// a target-driven furnace/balance, the experiment stage engine (heat → hold → reduce →
// cool → complete + safety), an in-memory experiment history, and the GB analyses.

import type {
  ReductionInput,
  RdiInput,
  SwellingInput,
  ReportDetails,
  ClampResult,
  ExperimentDetail,
  ExperimentParams,
  ExperimentState,
  ExperimentSummary,
  ProgramDto,
  ExperimentPreflightResult,
  ExperimentStartDefaults,
  SampleDto,
  StageDto,
  TareResult,
  TelemetrySnapshot,
} from "./types";
import { mockCurrentUser } from "./authMock";

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

const approach = (cur: number, tgt: number, frac: number) =>
  cur + (tgt - cur) * frac;

function clampGas(gas: number, value: number): number {
  if (!Number.isFinite(value) || value < 0) return 0; // mirror the backend clamp (#4)
  let limit = Number.POSITIVE_INFINITY;
  if (gas === 0) limit = Math.min(limits.h2, H2_ABS);
  if (gas === 3) limit = Math.min(limits.co, CO_ABS);
  return Math.min(value, limit);
}

export function tick(): TelemetrySnapshot {
  seq += 1;
  temps = temps.map(
    (t) => approach(t, furnaceTarget, 0.08) + (Math.random() - 0.5) * 0.6,
  );
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
    health: {
      safety_latched: safetyLatched,
      temp_valid: true,
      mfc_valid: true,
      balance_valid: true,
      temp_connected: true,
      mfc_connected: true,
      balance_connected: true,
    },
  };
}

export function mockSetFlow(gas: number, value: number): ClampResult {
  if (safetyLatched) throw new Error("保护已锁存，请先复位");
  const applied = clampGas(gas, value);
  const limit =
    gas === 0
      ? Math.min(limits.h2, H2_ABS)
      : gas === 3
        ? Math.min(limits.co, CO_ABS)
        : Infinity;
  sv[gas] = applied;
  return { applied, clamped: applied < value, limit };
}

export function mockSetOnOff(gas: number, on: boolean): void {
  if (safetyLatched) throw new Error("保护已锁存，请先复位");
  if (!on) sv[gas] = 0;
}

let safetyLatched = false;
export function mockShutoff(): void {
  safetyLatched = true;
  if (run) mockStopExperiment();
  sv[0] = 0;
  sv[1] = 5;
  sv[2] = 0;
  sv[3] = 0;
}
export function mockResetSafety(): void {
  if (!mockCurrentUser()) throw new Error("AUTH_REQUIRED");
  if (run) throw new Error("实验仍在结束");
  if ([0, 2, 3].some((i) => pv[i]! > 0.1))
    throw new Error("等待气体流量归零后复位");
  safetyLatched = false;
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
const customPrograms: ProgramDto[] = [];
const customRevisions = new Map<string, ProgramDto[]>();
const programGasLimits = { h2: 5, co: 5, flow_steps: [0.01, 0.1, 0.01, 0.01] };
const PROGRAMS = standardPrograms as ProgramDto[];

export function mockListPrograms(): ProgramDto[] {
  return structuredClone([...PROGRAMS, ...customPrograms]);
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
  if (!Number.isFinite(t8)) return false;
  const atTemp = Math.abs(t8 - s.target_temp) <= s.tolerance;
  switch (s.kind) {
    case "MANUAL":
      return false;
    case "COOLING":
      return t8 < s.target_temp;
    case "PURGING":
    case "REDUCING":
      return elapsedS >= s.duration_min * 60;
    case "STABILIZING":
      return atTemp && elapsedS >= s.duration_min * 60;
    case "HEATING":
      return atTemp;
  }
}

function applyStage(s: StageDto, w0: number): void {
  s.gas.forEach((v, i) => (sv[i] = clampGas(i, v)));
  furnaceTarget = ["COOLING", "MANUAL"].includes(s.kind) ? 25 : s.target_temp;
  if (s.kind === "REDUCING") weightTarget = w0 * (1 - MOCK_LOSS);
}

interface MockRun {
  sampleId: string;
  projectName: string | null;
  program: ProgramDto;
  id: string;
  mode: string;
  sample: string;
  operator: string;
  ownerUserId: string;
  notes: string;
  started: string;
  w0: number;
  stageIndex: number;
  stageElapsedS: number;
  stableElapsedS: number;
  actions: Record<string, unknown>[];
  skipped: boolean;
  totalElapsedS: number;
  samples: SampleDto[];
  timer: ReturnType<typeof setInterval> | null;
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
    stable_elapsed_s: r.stableElapsedS,
    total_elapsed_s: r.totalElapsedS,
    owner_user_id: r.ownerUserId,
    owner_display_name: r.operator,
    can_control: true,
    occupied_by_other: false,
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
    experiment_status: r.program.stages[r.stageIndex]?.kind ?? "UNKNOWN",
  };
}

function archive(r: MockRun, completed = true, outcome = "completed"): void {
  const detail: ExperimentDetail = {
    experiment_id: r.id,
    experiment_name: r.sample,
    sample_id: r.sampleId,
    project_name: r.projectName,
    experiment_time: r.started,
    program_snapshot: structuredClone(r.program),
    sample_name: r.sample,
    sample_weight: r.w0,
    start_time: r.started,
    end_time: completed ? new Date().toISOString() : null,
    description: r.notes || null,
    operator: r.operator || null,
    experiment_type: r.mode,
    analysis: null,
    created_at: r.started,
    sample_count: r.samples.length,
    runtime: {
      device_mode: "browser-mock",
      outcome,
      skipped: r.skipped,
      stage_actions: structuredClone(r.actions),
    },
    owner_user_id: r.ownerUserId,
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

  run.stableElapsedS =
    Number.isFinite(t8) && Math.abs(t8 - stage.target_temp) <= stage.tolerance
      ? run.stableElapsedS + SIM_DT
      : 0;
  if (
    stageReached(
      stage,
      t8,
      stage.kind === "STABILIZING" ? run.stableElapsedS : run.stageElapsedS,
    )
  ) {
    advanceRun();
    return;
  }
  experiment = runningState(run);
  notify();
}

function advanceRun(): void {
  if (!run) return;
  run.stageIndex += 1;
  run.stageElapsedS = 0;
  run.stableElapsedS = 0;
  const next = run.program.stages[run.stageIndex];
  if (!next) {
    completeRun();
    return;
  }
  applyStage(next, run.w0);
  experiment = runningState(run);
  notify();
}

function completeRun(): void {
  if (!run) return;
  // The last acknowledged standard stage defines the normal ending atmosphere.
  furnaceTarget = 25;
  weightTarget = weight;
  archive(run);
  stopRun();
  experiment = { running: false };
  notify();
}

export function mockStartExperiment(p: ExperimentParams): string {
  if (safetyLatched) throw new Error("保护已锁存，请先复位");
  if (!p.sample_id.trim()) throw new Error("样品编号必填");
  if (!p.sample_name.trim()) throw new Error("样品名称必填");
  if (!(p.sample_weight > 0)) throw new Error("样品重量必须大于 0");
  const user = mockCurrentUser();
  if (!user) throw new Error("AUTH_REQUIRED");
  if (experiment.running) throw new Error("实验已在运行");
  const program = mockListPrograms().find((pr) => pr.id === p.experiment_type);
  if (!program) throw new Error("未知实验模式");
  const error =
    executableProgramError(program) ??
    gasConditionErrors(program, programGasLimits)[0];
  if (error) throw new Error(error);
  if (!program.enabled) throw new Error("实验模式已停用");

  const id = `exp-${Date.now()}`;
  const started = new Date().toISOString();
  run = {
    program: { ...program, stages: program.stages.map((s) => ({ ...s })) },
    id,
    mode: p.experiment_type,
    sample: p.sample_name,
    sampleId: p.sample_id,
    projectName: p.project_name ?? null,
    operator: user.display_name,
    ownerUserId: user.user_id,
    notes: p.notes ?? "",
    started,
    w0: p.sample_weight,
    stageIndex: 0,
    stageElapsedS: 0,
    stableElapsedS: 0,
    actions: [],
    skipped: false,
    totalElapsedS: 0,
    samples: [],
    timer: null,
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
    archive(run, true, "stopped"); // interrupted records must not claim normal completion
    stopRun();
  }
  experiment = { running: false };
  notify();
}

function applyOperatorAction(
  experiment_id: string,
  stage_index: number,
  notes: string,
  kind: "skip" | "confirm",
): void {
  const user = mockCurrentUser();
  if (!user) throw new Error("AUTH_REQUIRED");
  if (!run || experiment_id !== run.id || stage_index !== run.stageIndex)
    throw new Error("实验或阶段已变化");
  if (user.role !== "ADMIN" && user.user_id !== run.ownerUserId)
    throw new Error("FORBIDDEN");
  if (notes.length > 4096) throw new Error("操作备注不能超过 4096 字");
  if (kind === "skip" && !notes.trim()) throw new Error("跳段原因必填");
  const stage = run.program.stages[stage_index]!;
  if (
    kind === "confirm"
      ? stage.kind !== "MANUAL"
      : !["HEATING", "STABILIZING", "REDUCING"].includes(stage.kind) ||
        !notes.trim()
  )
    throw new Error("此阶段不能执行该操作");
  run.actions.push({
    experiment_id,
    stage_index,
    kind,
    notes: notes.trim(),
    user_id: user.user_id,
    operator: user.display_name,
    confirmed_at: new Date().toISOString(),
    elapsed_s: run.totalElapsedS,
  });
  if (kind === "skip") run.skipped = true;
  advanceRun();
}
export function mockSkipStage(
  experiment_id: string,
  stage_index: number,
  reason: string,
): void {
  applyOperatorAction(experiment_id, stage_index, reason, "skip");
}
export function mockAcknowledgeStage(
  experiment_id: string,
  stage_index: number,
  notes: string,
): void {
  applyOperatorAction(experiment_id, stage_index, notes, "confirm");
}

export function mockSubscribeExperiment(
  cb: (s: ExperimentState) => void,
): () => void {
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

function accessible(item: HistItem): boolean {
  const user = mockCurrentUser();
  return Boolean(
    user &&
      (user.role === "ADMIN" || item.detail.owner_user_id === user.user_id),
  );
}

function historyItem(id: string): HistItem {
  const item = history.get(id);
  if (!item) throw new Error("实验不存在");
  if (!accessible(item)) throw new Error("FORBIDDEN");
  return item;
}

/** Synchronously simulate a full run to fabricate a seed history entry. */
function simulate(program: ProgramDto, w0: number): SampleDto[] {
  let t = 25;
  let wt = w0;
  let stageIndex = 0;
  let stageEl = 0;
  let stableEl = 0;
  let totalEl = 0;
  const samples: SampleDto[] = [];
  let guard = 0;
  while (stageIndex < program.stages.length && guard++ < 5000) {
    const s = program.stages[stageIndex];
    if (!s) break;
    t +=
      ((["COOLING", "MANUAL"].includes(s.kind) ? 25 : s.target_temp) - t) * 0.1;
    if (s.kind === "REDUCING") wt += (w0 * (1 - MOCK_LOSS) - wt) * 0.06;
    stageEl += 60;
    totalEl += 60;
    samples.push({
      timestamp: new Date(
        Date.parse("2026-06-01T09:00:00") + totalEl * 1000,
      ).toISOString(),
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
    stableEl = Math.abs(t - s.target_temp) <= s.tolerance ? stableEl + 60 : 0;
    if (
      s.kind === "MANUAL" ||
      stageReached(s, t, s.kind === "STABILIZING" ? stableEl : stageEl)
    ) {
      stageIndex += 1;
      stageEl = 0;
      stableEl = 0;
    }
  }
  return samples;
}

function seed(
  id: string,
  name: string,
  mode: string,
  start: string,
  w0: number,
): void {
  const program = mockListPrograms().find((p) => p.id === mode);
  if (!program) return;
  const samples = simulate(program, w0);
  history.set(id, {
    detail: {
      experiment_id: id,
      experiment_name: name,
      sample_name: name,
      sample_weight: w0,
      start_time: start,
      end_time: new Date(
        Date.parse(start) + samples.length * 60000,
      ).toISOString(),
      description: "演示数据",
      operator: "张工",
      experiment_type: mode,
      analysis: null,
      created_at: start,
      sample_count: samples.length,
      owner_user_id: null,
    },
    samples,
  });
}
seed("exp-demo-2", "鞍钢球团-B", "REDUCIBILITY", "2026-06-02T09:00:00", 100);
seed(
  "exp-demo-1",
  "首钢烧结矿-A",
  "LOW_TEMP_DEGRADATION",
  "2026-06-01T14:30:00",
  500,
);

export function mockListExperiments(): ExperimentSummary[] {
  return [...history.values()]
    .filter(accessible)
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
      owner_user_id: h.detail.owner_user_id,
    }))
    .sort((a, b) => b.start_time.localeCompare(a.start_time));
}

export function mockGetExperimentDetail(id: string): ExperimentDetail {
  if (run?.id === id) archive(run, false);
  const h = historyItem(id);
  return { ...h.detail, sample_count: h.samples.length };
}

export function mockGetExperimentSamples(
  id: string,
  maxPoints?: number,
): SampleDto[] {
  if (run?.id === id) archive(run, false);
  const h = historyItem(id);
  const all = h.samples;
  const cap = maxPoints ?? 2000;
  if (cap <= 0 || all.length <= cap) return all.map((s) => ({ ...s }));
  const stride = Math.ceil(all.length / cap);
  return all.filter((_, i) => i % stride === 0).map((s) => ({ ...s }));
}

export function mockDeleteExperiment(id: string): void {
  if (mockCurrentUser()?.role !== "ADMIN") throw new Error("FORBIDDEN");
  history.delete(id);
}

export function mockSetExperimentOwner(
  id: string,
  ownerUserId: string | null,
): void {
  if (mockCurrentUser()?.role !== "ADMIN") throw new Error("FORBIDDEN");
  const item = history.get(id);
  if (!item) throw new Error("实验不存在");
  item.detail.owner_user_id = ownerUserId;
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
  if (run?.id === id) archive(run, false);
  const h = historyItem(id);
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
  if (run?.id === id) archive(run, false);
  const esc = (value: string) =>
    value
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;");
  const h = historyItem(id);
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
    .map(([k, v]) => `<tr><th>${esc(k!)}</th><td>${esc(v!)}</td></tr>`)
    .join("");
  const analysis = a
    ? `<pre>${esc(JSON.stringify(a, null, 2))}</pre>`
    : '<p style="color:#999">尚未进行结果分析。</p>';
  return `<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><title>检测报告 ${d.experiment_id}</title>\
<style>body{font-family:'Microsoft YaHei',system-ui,sans-serif;max-width:820px;margin:24px auto;padding:0 16px;color:#1a1a1a}\
h1{font-size:20px}h2{font-size:14px;border-bottom:2px solid #d9631f;padding-bottom:4px;margin-top:24px}\
table{border-collapse:collapse}th,td{border:1px solid #e2e2e2;padding:5px 10px;text-align:left}th{background:#f6f6f6}\
pre{background:#f6f6f6;padding:10px;border-radius:4px;font-size:12px}</style></head><body>\
<h1>铁矿石冶金性能检测报告</h1><div style="color:#666;font-size:12px">TMH-LPF-900 · 北京科技大学（浏览器预览）</div>\
<h2>实验信息</h2><table>${meta}</table><h2>报告人工补录（浏览器模拟）</h2><pre>${esc(JSON.stringify(d.runtime?.report_details ?? "待补录", null, 2))}</pre>${programSnapshotHtml(d.program_snapshot)}<h2>结果分析</h2>${analysis}</body></html>`;
}

// ---- GB analyses (mirror tmh_core::gb) ----
export function mockAnalyze13241(
  id: string,
  input: ReductionInput,
): Record<string, unknown> {
  const h = historyItem(id);
  if (!h.detail.end_time) throw new Error("实验未结束");
  if (
    (h.detail.program_snapshot?.base_standard_id ??
      h.detail.experiment_type) !== "REDUCIBILITY"
  )
    throw new Error("分析方法不匹配");
  const endpoint =
    h.detail.program_snapshot?.stages.find((s) => s.kind === "REDUCING")
      ?.duration_min ?? 180;
  if (
    input.manual_m180 != null &&
    (endpoint !== 180 || input.manual_mt != null)
  )
    throw new Error("m₁₈₀ 仅适用于 180 分钟终点且不能与 mₜ 重复补录");
  const { manual_m1: m1, w1, w2 } = input;
  const m180 = input.manual_mt ?? input.manual_m180;
  if (m1 == null || m180 == null)
    throw new Error(
      "浏览器旧模拟记录缺少还原称量，请依据原始记录补录 m₁ 和 mₜ",
    );
  if (!input.source_record?.trim())
    throw new Error("人工补录须填写原始记录编号或来源说明");
  const m0 = h.detail.sample_weight;
  if (
    ![m0, m1, m180, w1, w2].every(Number.isFinite) ||
    m0 <= 0 ||
    m1 < m0 ||
    m180 <= 0 ||
    w1 < 0 ||
    w1 > 100 ||
    w2 <= 0 ||
    w2 > 100
  )
    throw new Error("亚铁/全铁含量或称量无效");
  const points = input.manual_rate_points;
  let rate: Record<string, number> | null = null;
  if (points) {
    if (
      points.some(
        ([t, y], i) =>
          !Number.isFinite(t) ||
          !Number.isFinite(y) ||
          t < 0 ||
          t > endpoint ||
          (i > 0 && points[i - 1]![0] >= t),
      )
    )
      throw new Error("人工速率曲线须按时间递增");
    const crossing = (threshold: number): number | null => {
      for (let i = 0; i < points.length; i++) {
        const [t, y] = points[i]!;
        if (y === threshold) return t;
        const previous = points[i - 1];
        if (previous && previous[1] < threshold && y > threshold)
          return (
            previous[0] +
            ((threshold - previous[1]) / (y - previous[1])) * (t - previous[0])
          );
      }
      return null;
    };
    const t30 = crossing(30);
    for (const [endpoint, factor] of [
      [60, 33.6],
      [55, 26.5],
      [50, 20],
    ] as const) {
      const t = crossing(endpoint);
      if (t30 !== null && t !== null && t > t30) {
        rate = {
          index: factor / (t - t30),
          t30,
          endpoint_percent: endpoint,
          endpoint_min: t,
          factor,
        };
        break;
      }
    }
    if (!rate)
      throw new Error("人工速率曲线须覆盖 30% 与 60%、55% 或 50% 还原度");
  }
  const rt =
    ((0.111 * w1) / (0.43 * w2) + ((m1 - m180) / (m0 * 0.43 * w2)) * 100) * 100;
  const value = {
    standard: "GB/T 13241",
    standard_version: "2017",
    calculation_version: "GB-2026-09-05.1",
    input: structuredClone(input),
    m0,
    m1,
    rt,
    ...(h.detail.program_snapshot?.source === "custom" ? {} : { ri: rt }),
    endpoint_min: endpoint,
    reduction_rate_index: rate?.index ?? null,
    rate_evidence: rate,
    manual_used: true,
    result_scope: "single_trial",
    standard_conformity:
      h.detail.program_snapshot?.source === "custom"
        ? "custom_conditions"
        : "pending_group_and_process_verification",
    computed_by: mockCurrentUser()?.display_name,
    computed_at: new Date().toISOString(),
  };
  h.detail.analysis = value;
  h.detail.revisions = [structuredClone(value), ...(h.detail.revisions ?? [])];
  return value;
}
export function mockAnalyze13242(
  id: string,
  input: RdiInput,
): Record<string, unknown> {
  const h = historyItem(id);
  if (!h.detail.end_time) throw new Error("实验未结束");
  if (
    !Number.isFinite(input.drum_rpm) ||
    input.drum_rpm <= 0 ||
    !Number.isInteger(input.drum_revolutions) ||
    input.drum_revolutions <= 0 ||
    !Number.isFinite(input.sieve_seconds) ||
    input.sieve_seconds <= 0
  )
    throw new Error("请填写实际转鼓与筛分记录");
  const { m0, m1, m2, m3 } = input;
  if (
    ![m0, m1, m2, m3].every((x) => Number.isFinite(x) && x >= 0) ||
    m0 <= 0 ||
    m1 + m2 + m3 > m0
  )
    throw new Error("入鼓/筛上质量无效");
  if (
    (h.detail.program_snapshot?.base_standard_id ??
      h.detail.experiment_type) !== "LOW_TEMP_DEGRADATION"
  )
    throw new Error("分析方法不匹配");
  const value = {
    standard: "GB/T 13242",
    calculation_version: "GB-2026-09-05.1",
    input,
    rdi_plus_6_3: (m1 / m0) * 100,
    rdi_plus_3_15: ((m1 + m2) / m0) * 100,
    rdi_minus_0_5: ((m0 - m1 - m2 - m3) / m0) * 100,
    result_scope: "single_trial",
    external_conditions_met:
      Math.abs(input.drum_rpm - 30) <= 1 &&
      input.drum_revolutions === 300 &&
      input.sieve_seconds === 60,
  };
  h.detail.analysis = value;
  return value;
}
export function mockAnalyze13240(
  id: string,
  input: SwellingInput,
): Record<string, unknown> {
  const h = historyItem(id);
  if (!h.detail.end_time) throw new Error("实验未结束");
  if (
    ![input.v0, input.v1].every((x) => Number.isFinite(x) && x > 0) ||
    input.pellet_count !== 18 ||
    !input.volume_method.trim() ||
    !input.same_method
  )
    throw new Error("整份体积、颗粒数或体积方法无效");
  if (
    (h.detail.program_snapshot?.base_standard_id ??
      h.detail.experiment_type) !== "FREE_SWELLING"
  )
    throw new Error("分析方法不匹配");
  const value = {
    standard: "GB/T 13240",
    calculation_version: "GB-2026-09-05.1",
    input,
    swelling_index: ((input.v1 - input.v0) / input.v0) * 100,
    result_scope: "single_trial",
  };
  h.detail.analysis = value;
  return value;
}

function modeAdmin(): void {
  const user = mockCurrentUser();
  if (!user) throw new Error("AUTH_REQUIRED");
  if (user.role !== "ADMIN") throw new Error("FORBIDDEN");
  if (run) throw new Error("实验运行中不可修改模式");
}
export function mockSaveCustomProgram(
  program: ProgramDto,
  reason: string,
): ProgramDto {
  modeAdmin();
  if (!reason?.trim() || reason.length > 4096)
    throw new Error("请填写修改原因（1—4096 字）");
  if (PROGRAMS.some((p) => p.id === program.id))
    throw new Error("标准模式不可覆盖");
  const p = structuredClone(program);
  p.id ||= `custom-${crypto.randomUUID()}`;
  if (!p.id.startsWith("custom-")) throw new Error("自定义程序编号无效");
  p.source = "custom";
  const i = customPrograms.findIndex((x) => x.id === p.id);
  const previous = customPrograms[i];
  if (!previous && customRevisions.has(p.id))
    throw new Error("已删除程序不能复用编号");
  if (
    previous &&
    (previous.base_standard_id !== p.base_standard_id ||
      previous.provenance?.revision !== p.provenance?.revision)
  )
    throw new Error("程序版本已变化或基础标准不匹配，请重新读取后编辑");
  const parent = previous?.provenance?.parent_template ?? standardParent(p);
  const error =
    programValueErrors(p)[0] ??
    customShapeError(p, parent) ??
    gasConditionErrors(p, programGasLimits)[0];
  if (error) throw new Error(error);
  if (!parent) throw new Error("基础国标未知");
  const user = mockCurrentUser()!;
  p.provenance = {
    revision: (previous?.provenance?.revision ?? 0) + 1,
    parent_template: structuredClone(parent),
    edited_by_user_id: user.user_id,
    edited_by: user.display_name,
    edited_at: new Date().toISOString(),
    reason: reason.trim(),
    changes: conditionChanges(p, parent),
  };
  const executionError = executableProgramError(p);
  if (executionError) throw new Error(executionError);
  if (i < 0) customPrograms.push(p);
  else customPrograms[i] = p;
  customRevisions.set(p.id, [
    structuredClone(p),
    ...(customRevisions.get(p.id) ??
      (previous ? [structuredClone(previous)] : [])),
  ]);
  return structuredClone(p);
}
export function mockListCustomProgramRevisions(id: string): ProgramDto[] {
  if (!mockCurrentUser()) throw new Error("AUTH_REQUIRED");
  return structuredClone(customRevisions.get(id) ?? []);
}
export function mockDeleteCustomProgram(id: string): void {
  modeAdmin();
  const i = customPrograms.findIndex((p) => p.id === id);
  if (i < 0) throw new Error("自定义模式不存在");
  customPrograms.splice(i, 1);
}
export function mockGenerateSampleId(
  programId: string,
  experimentTime: string,
): string {
  const p = mockListPrograms().find((x) => x.id === programId);
  if (!p) throw new Error("未知实验模式");
  const prefix = `${p.sample_prefix}-${experimentTime.slice(0, 10).replaceAll("-", "")}-`;
  const max = Math.max(
    0,
    ...mockListExperiments().map((r) =>
      r.sample_id?.startsWith(prefix)
        ? Number(r.sample_id.slice(prefix.length)) || 0
        : 0,
    ),
  );
  return `${prefix}${String(max + 1).padStart(3, "0")}`;
}
export function mockGetExperimentStartDefaults(
  programId: string,
): ExperimentStartDefaults {
  const user = mockCurrentUser();
  if (!user) throw new Error("AUTH_REQUIRED");
  const latest = mockListExperiments()[0];
  const now = new Date().toISOString();
  return {
    sample_id: mockGenerateSampleId(programId, now),
    sample_name: latest?.sample_name ?? "",
    sample_weight: 500,
    operator: user.display_name,
    experiment_type: programId,
    experiment_time: now,
    project_name: latest?.project_name ?? null,
    notes: null,
  };
}
export function mockPreflightExperiment(
  params: ExperimentParams,
): ExperimentPreflightResult {
  if (!mockCurrentUser()) throw new Error("AUTH_REQUIRED");
  const program =
    mockListPrograms().find((p) => p.id === params.experiment_type) ?? null;
  const blocking = [];
  if (run) blocking.push({ code: "RUNNING", message_zh: "实验已在运行" });
  if (
    !params.sample_id?.trim() ||
    !params.sample_name.trim() ||
    !Number.isFinite(params.sample_weight) ||
    params.sample_weight <= 0
  )
    blocking.push({
      code: "PARAMS",
      message_zh: "请填写有效样品编号、名称与重量",
    });
  if (!program?.enabled)
    blocking.push({ code: "PROGRAM", message_zh: "实验模式不可用" });
  if (program) {
    const error = executableProgramError(program);
    if (error) blocking.push({ code: "PROGRAM_INVALID", message_zh: error });
    for (const message_zh of gasConditionErrors(program, programGasLimits))
      blocking.push({ code: "PROGRAM_GAS_INVALID", message_zh });
  }
  return { ok: !blocking.length, blocking, warnings: [], program };
}

export function mockSaveReportDetails(
  id: string,
  details: ReportDetails,
): void {
  const h = historyItem(id);
  if (Object.values(details).some((v) => v.length > 4096))
    throw new Error("报告字段过长");
  const patch = {
    report_details: structuredClone(details),
    report_details_operator: mockCurrentUser()?.display_name,
    report_details_updated_at: new Date().toISOString(),
  };
  h.detail.runtime = { ...h.detail.runtime, ...patch };
  h.detail.revisions = [structuredClone(patch), ...(h.detail.revisions ?? [])];
}
