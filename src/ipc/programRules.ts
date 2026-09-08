import type { StageDto } from "./types";

/** Mirror the native engine's explicit timed/dynamic stage contract. */
export function validStageDuration(stage: StageDto): boolean {
  if (!Number.isFinite(stage.duration_min) || stage.duration_min < 0)
    return false;
  return ["STABILIZING", "REDUCING", "PURGING"].includes(stage.kind)
    ? stage.duration_min > 0
    : stage.duration_min === 0;
}

import standards from "../../config/standard_programs.json";
import type { GasLimits, ProgramChange, ProgramDto } from "./types";

const fields: (keyof StageDto)[] = [
  "name",
  "description",
  "kind",
  "target_temp",
  "tolerance",
  "duration_min",
  "ramp_rate",
  "gas",
];
export function standardParent(program: ProgramDto): ProgramDto | undefined {
  return (standards as ProgramDto[]).find(
    (p) => p.id === program.base_standard_id,
  );
}
export function conditionChanges(
  program: ProgramDto,
  parent: ProgramDto,
): ProgramChange[] {
  const changes: ProgramChange[] = [];
  program.stages.forEach((stage, stage_index) => {
    const base = parent.stages[stage_index];
    if (!base) return;
    for (const field of fields)
      if (JSON.stringify(stage[field]) !== JSON.stringify(base[field])) {
        changes.push({
          stage_index,
          field,
          before: base[field],
          after: stage[field],
        });
      }
  });
  return changes;
}
export function customShapeError(
  program: ProgramDto,
  parent = standardParent(program),
): string | null {
  if (
    !parent ||
    !parent.template_revision ||
    program.base_standard_id !== parent.id ||
    program.base_standard_version !== parent.base_standard_version ||
    program.template_revision !== parent.template_revision
  )
    return "基础标准年份或模板版本不匹配，请从当前国标模板复制";
  if (
    program.stages.length !== parent.stages.length ||
    program.stages.some((s, i) => s.kind !== parent.stages[i]?.kind)
  )
    return "首版自定义保留国标阶段顺序和类型";
  if (conditionChanges(program, parent).some((c) => c.stage_index >= 3))
    return "国标结束步骤不可更改或省略";
  return null;
}
export function gasConditionErrors(
  program: ProgramDto,
  limits: GasLimits,
): string[] {
  const errors: string[] = [];
  program.stages.forEach((stage, i) => {
    if ((stage.gas[0] ?? 0) > Math.min(limits.h2, 10))
      errors.push(
        `第 ${i + 1} 阶段 H₂ 超过安全上限 ${Math.min(limits.h2, 10)} L/min`,
      );
    if ((stage.gas[3] ?? 0) > Math.min(limits.co, 10))
      errors.push(
        `第 ${i + 1} 阶段 CO 超过安全上限 ${Math.min(limits.co, 10)} L/min`,
      );
    limits.flow_steps?.forEach((step, g) => {
      const raw = (stage.gas[g] ?? Number.NaN) / step;
      if (
        step <= 0 ||
        !Number.isFinite(raw) ||
        raw < 0 ||
        raw >= 2 ** 63 ||
        Math.abs(raw - Math.round(raw)) > 1e-8
      )
        errors.push(
          `第 ${i + 1} 阶段 ${["H₂", "N₂", "CO₂", "CO"][g]} 流量须为 ${step} L/min 的整数倍且在协议范围内`,
        );
    });
  });
  return errors;
}
/** Mock and UI use the same structural rules; native code validates again. */
export function executableProgramError(program: ProgramDto): string | null {
  if (program.stages.some((s) => !validStageDuration(s)))
    return "阶段类型与时长不匹配";
  if (program.source === "standard") return null;
  if (!program.provenance)
    return "旧自定义程序缺少版本依据，请从当前国标重新复制";
  const error = customShapeError(program);
  const parent = standardParent(program);
  if (error || !parent) return error ?? "基础国标未知";
  const saved = program.provenance.parent_template;
  if (
    customShapeError(saved, parent) ||
    saved.id !== parent.id ||
    saved.source !== "standard" ||
    conditionChanges(saved, parent).length
  )
    return "基础模板已有更新，请从当前国标另建程序";
  if (
    program.provenance.revision <= 0 ||
    JSON.stringify(program.provenance.changes) !==
      JSON.stringify(conditionChanges(program, parent))
  )
    return "自定义版本或条件差异记录无效，请重新保存";
  return null;
}

export function programValueErrors(program: ProgramDto): string[] {
  const errors: string[] = [];
  if (!program.name.trim()) errors.push("模式名称必填");
  if (!/^[A-Z0-9]{2,8}$/.test(program.sample_prefix))
    errors.push("样品编号前缀需为 2-8 位字母或数字");
  if (!program.stages.length) errors.push("至少需要一个阶段");
  program.stages.forEach((s, i) => {
    if (!s.name.trim()) errors.push(`第 ${i + 1} 阶段名称必填`);
    if (!Number.isFinite(s.target_temp) || !Number.isFinite(s.ramp_rate))
      errors.push(`第 ${i + 1} 阶段温度或升温率无效`);
    if (!Number.isFinite(s.tolerance) || s.tolerance < 0)
      errors.push(`第 ${i + 1} 阶段容差必须为非负数`);
    if (!validStageDuration(s)) errors.push(`第 ${i + 1} 阶段类型与时长不匹配`);
    const total = s.gas.reduce((a, b) => a + b, 0);
    if (
      s.gas.length !== 4 ||
      s.gas.some((v) => !Number.isFinite(v) || v < 0) ||
      !Number.isFinite(total) ||
      (s.kind !== "MANUAL" && total <= 0)
    )
      errors.push(`第 ${i + 1} 阶段气体流量无效`);
  });
  return errors;
}
