import type {
  GasLimits,
  ProgramDto,
  StageDto,
  StageKind,
} from "../../ipc/types";

export const STAGE_KIND_LABEL: Record<StageKind, string> = {
  HEATING: "升温",
  STABILIZING: "恒温",
  REDUCING: "还原",
  COOLING: "冷却",
  PURGING: "吹扫",
  MANUAL: "人工确认",
};

export const STAGE_KIND_COLOR: Record<StageKind, string> = {
  HEATING: "var(--heat)",
  STABILIZING: "var(--warn)",
  REDUCING: "var(--gas)",
  COOLING: "var(--ok)",
  PURGING: "var(--gas)",
  MANUAL: "var(--warn)",
};

export const GAS_LABELS = ["H₂", "N₂", "CO₂", "CO"] as const;

export type ProgramValidation = {
  errors: string[];
  warnings: string[];
};

export type DryRunStep = {
  stage: string;
  condition: string;
  atmosphere: string;
  target: string;
};

const stageDefaults: StageDto = {
  name: "升温",
  kind: "HEATING",
  gas: [0, 5, 0, 0],
  target_temp: 900,
  tolerance: 5,
  duration_min: 0,
  ramp_rate: 10,
  description: "N₂保护升温",
};

export function samplePrefixFallback(id: string): string {
  if (id === "REDUCIBILITY") return "RED";
  if (id === "LOW_TEMP_DEGRADATION") return "RDI";
  if (id === "FREE_SWELLING") return "FSI";
  return "CUS";
}

export function normalizeSamplePrefix(value: string, id = ""): string {
  const cleaned = value
    .replace(/[^a-z0-9]/gi, "")
    .slice(0, 8)
    .toUpperCase();
  return cleaned.length >= 2 ? cleaned : samplePrefixFallback(id);
}

export function cloneProgram(program: ProgramDto): ProgramDto {
  return {
    ...program,
    description: program.description ?? "",
    category:
      program.category?.trim() ||
      (program.source === "standard" ? "standard" : "custom"),
    enabled: program.enabled ?? true,
    sample_prefix: normalizeSamplePrefix(
      program.sample_prefix ?? "",
      program.id,
    ),
    stages: program.stages.map((s) => ({ ...s, gas: normalizeGas(s.gas) })),
  };
}

export function normalizeGas(gas: number[]): number[] {
  return [0, 1, 2, 3].map((i) => Number(gas[i] ?? 0));
}

export function gasSummary(gas: number[]): string {
  const parts = normalizeGas(gas)
    .map((v, i) => {
      const label = GAS_LABELS[i] ?? `G${i + 1}`;
      return v > 0 ? `${formatNumber(v)}${label}` : null;
    })
    .filter(Boolean);
  return parts.length ? parts.join(" + ") : "无";
}

export function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

export function fixedDurationMin(program: ProgramDto): number {
  return program.stages.reduce(
    (total, stage) => total + Math.max(0, stage.duration_min),
    0,
  );
}

export function dynamicStageCount(program: ProgramDto): number {
  return program.stages.filter((stage) => stage.duration_min === 0).length;
}

export function durationSummary(program: ProgramDto): string {
  const fixed = fixedDurationMin(program);
  const dynamic = dynamicStageCount(program);
  const fixedText = fixed > 0 ? `固定 ${fixed} min` : "无固定保温";
  return dynamic > 0 ? `${fixedText} / ${dynamic} 个动态阶段` : fixedText;
}

export function stageCondition(stage: StageDto): string {
  if (stage.kind === "MANUAL") return stage.description || "等待操作员确认";
  if (stage.kind === "COOLING") return `T8 < ${stage.target_temp}℃`;
  if (stage.kind === "PURGING")
    return `吹扫 ${stage.duration_min} min，气氛确认后计时`;
  if (stage.kind === "REDUCING")
    return `还原 ${stage.duration_min} min，T8 ${stage.target_temp}℃±${stage.tolerance}；偏离温度仍按固定时长结束并记录偏离`;
  if (stage.kind === "STABILIZING")
    return `T8 ${stage.target_temp}℃±${stage.tolerance}，连续保持 ${stage.duration_min} min；超差重新计时`;
  return `T8 ${stage.target_temp}℃±${stage.tolerance}`;
}

export function createBlankCustomProgram(): ProgramDto {
  return {
    id: "",
    name: "自定义实验模式",
    description: "自定义阶段程序",
    category: "custom",
    enabled: true,
    sample_prefix: "CUS",
    source: "custom",
    stages: [{ ...stageDefaults, gas: [...stageDefaults.gas] }],
  };
}

export function copyAsCustom(program: ProgramDto): ProgramDto {
  return {
    ...cloneProgram(program),
    id: "",
    name: `自定义_${program.name}`,
    description: `从 ${program.name} 复制的自定义实验模式`,
    category: "custom",
    enabled: true,
    sample_prefix: normalizeSamplePrefix(program.sample_prefix, program.id),
    source: "custom",
    base_standard_id: program.base_standard_id ?? program.id,
    base_standard_version:
      program.base_standard_version ??
      (program.id === "FREE_SWELLING" ? "2018" : "2017"),
  };
}

export function blankStage(index: number): StageDto {
  return {
    ...stageDefaults,
    name: `阶段 ${index + 1}`,
    gas: [...stageDefaults.gas],
  };
}

export function validateProgramDraft(
  program: ProgramDto,
  limits?: GasLimits | null,
): ProgramValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  if (!program.name.trim()) errors.push("模式名称必填");
  if (!/^[A-Z0-9]{2,8}$/.test(program.sample_prefix))
    errors.push("样品编号前缀需为 2-8 位字母或数字");
  if (program.stages.length === 0) errors.push("至少需要一个阶段");

  program.stages.forEach((stage, i) => {
    const n = i + 1;
    if (!stage.name.trim()) errors.push(`第 ${n} 阶段名称必填`);
    if (!Number.isFinite(stage.target_temp))
      errors.push(`第 ${n} 阶段目标温度无效`);
    if (!Number.isFinite(stage.tolerance) || stage.tolerance < 0)
      errors.push(`第 ${n} 阶段容差必须为非负数`);
    if (!Number.isFinite(stage.duration_min) || stage.duration_min < 0)
      errors.push(`第 ${n} 阶段时长必须为非负数`);
    if (!Number.isFinite(stage.ramp_rate))
      errors.push(`第 ${n} 阶段升温速率无效`);
    const gas = normalizeGas(stage.gas);
    if (gas.some((v) => !Number.isFinite(v) || v < 0))
      errors.push(`第 ${n} 阶段气体流量必须为非负数`);
    if (stage.kind !== "MANUAL" && gas.reduce((a, b) => a + b, 0) <= 0)
      errors.push(`第 ${n} 阶段总流量必须大于 0`);
    const h2 = gas[0] ?? 0;
    const co = gas[3] ?? 0;
    if (limits && h2 > limits.h2)
      warnings.push(`第 ${n} 阶段 H₂ 超过安全上限 ${limits.h2} L/min`);
    if (limits && co > limits.co)
      warnings.push(`第 ${n} 阶段 CO 超过安全上限 ${limits.co} L/min`);
  });

  return { errors, warnings };
}

export function sanitizeProgram(program: ProgramDto): ProgramDto {
  return {
    ...cloneProgram(program),
    id: program.id.trim(),
    name: program.name.trim(),
    description: program.description.trim(),
    category: program.category.trim() || "custom",
    enabled: program.enabled,
    sample_prefix: normalizeSamplePrefix(program.sample_prefix, program.id),
    source: "custom",
    stages: program.stages.map((stage) => ({
      ...stage,
      name: stage.name.trim(),
      gas: normalizeGas(stage.gas),
      description: stage.description.trim(),
    })),
  };
}

export function simulateProgram(program: ProgramDto): DryRunStep[] {
  const steps = program.stages.map((stage, i) => ({
    stage: `${i + 1}. ${stage.name}`,
    condition: stageCondition(stage),
    atmosphere: gasSummary(stage.gas),
    target: `${stage.target_temp}℃`,
  }));
  steps.push({
    stage: "完成",
    condition: "完成全部步骤后保留末阶段气氛；停止或故障切换 N₂ 5 L/min 保护",
    atmosphere: gasSummary(program.stages.at(-1)?.gas ?? []),
    target: "按仪表和人工操作记录",
  });
  return steps;
}
