import type { GasLimits, ProgramDto, StageDto, StageKind } from "../../ipc/types";

export const STAGE_KIND_LABEL: Record<StageKind, string> = {
  HEATING: "升温",
  STABILIZING: "恒温",
  REDUCING: "还原",
  COOLING: "冷却",
};

export const STAGE_KIND_COLOR: Record<StageKind, string> = {
  HEATING: "var(--heat)",
  STABILIZING: "var(--warn)",
  REDUCING: "var(--gas)",
  COOLING: "var(--ok)",
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

export function cloneProgram(program: ProgramDto): ProgramDto {
  return {
    ...program,
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
      return v > 0 ? `${label} ${formatNumber(v)}` : null;
    })
    .filter(Boolean);
  return parts.length ? parts.join(" · ") : "—";
}

export function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

export function fixedDurationMin(program: ProgramDto): number {
  return program.stages.reduce((total, stage) => total + Math.max(0, stage.duration_min), 0);
}

export function dynamicStageCount(program: ProgramDto): number {
  return program.stages.filter((stage) => stage.duration_min === 0).length;
}

export function durationSummary(program: ProgramDto): string {
  const fixed = fixedDurationMin(program);
  const dynamic = dynamicStageCount(program);
  const fixedText = fixed > 0 ? `固定保温/还原 ${fixed} min` : "无固定保温";
  return dynamic > 0 ? `${fixedText} · ${dynamic} 个动态阶段按 T8 判定` : fixedText;
}

export function stageCondition(stage: StageDto): string {
  if (stage.duration_min > 0) return `T8 ${stage.target_temp}℃±${stage.tolerance} 且 ${stage.duration_min} min`;
  if (stage.kind === "COOLING") return `T8 ≤ ${stage.target_temp + 5}℃`;
  return `T8 ${stage.target_temp}℃±${stage.tolerance}`;
}

export function createBlankCustomProgram(): ProgramDto {
  return {
    id: "",
    name: "自定义实验模式",
    source: "custom",
    stages: [{ ...stageDefaults, gas: [...stageDefaults.gas] }],
  };
}

export function copyAsCustom(program: ProgramDto): ProgramDto {
  return {
    ...cloneProgram(program),
    id: "",
    name: `${program.name} 副本`,
    source: "custom",
  };
}

export function blankStage(index: number): StageDto {
  return {
    ...stageDefaults,
    name: `阶段 ${index + 1}`,
    gas: [...stageDefaults.gas],
  };
}

export function validateProgramDraft(program: ProgramDto, limits?: GasLimits | null): ProgramValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  if (!program.name.trim()) errors.push("模式名称必填");
  if (program.stages.length === 0) errors.push("至少需要一个阶段");

  program.stages.forEach((stage, i) => {
    const n = i + 1;
    if (!stage.name.trim()) errors.push(`第 ${n} 阶段名称必填`);
    if (!Number.isFinite(stage.target_temp)) errors.push(`第 ${n} 阶段目标温度无效`);
    if (!Number.isFinite(stage.tolerance) || stage.tolerance < 0) errors.push(`第 ${n} 阶段容差必须为非负数`);
    if (!Number.isFinite(stage.duration_min) || stage.duration_min < 0) errors.push(`第 ${n} 阶段时长必须为非负数`);
    if (!Number.isFinite(stage.ramp_rate)) errors.push(`第 ${n} 阶段升温率无效`);
    const gas = normalizeGas(stage.gas);
    if (gas.some((v) => !Number.isFinite(v) || v < 0)) errors.push(`第 ${n} 阶段气体流量必须为非负数`);
    if (gas.reduce((a, b) => a + b, 0) <= 0) errors.push(`第 ${n} 阶段总流量必须大于 0`);
    const h2 = gas[0] ?? 0;
    const co = gas[3] ?? 0;
    if (limits && h2 > limits.h2) warnings.push(`第 ${n} 阶段 H₂ 超过安全上限 ${limits.h2} L/min`);
    if (limits && co > limits.co) warnings.push(`第 ${n} 阶段 CO 超过安全上限 ${limits.co} L/min`);
  });

  return { errors, warnings };
}

export function sanitizeProgram(program: ProgramDto): ProgramDto {
  return {
    ...cloneProgram(program),
    id: program.id.trim(),
    name: program.name.trim(),
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
    condition: "退出路径执行保护气氛",
    atmosphere: "N₂ 5 · H₂/CO₂/CO 0",
    target: "25℃",
  });
  return steps;
}
