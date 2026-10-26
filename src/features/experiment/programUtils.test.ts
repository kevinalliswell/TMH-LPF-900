import { describe, expect, it } from "vitest";
import type { ProgramDto } from "../../ipc/types";
import {
  copyAsCustom,
  durationSummary,
  gasSummary,
  normalizeSamplePrefix,
  simulateProgram,
  validateProgramDraft,
} from "./programUtils";

const program: ProgramDto = {
  id: "REDUCIBILITY",
  name: "GB/T 13241 还原性",
  description: "标准还原性试验",
  category: "standard",
  enabled: true,
  sample_prefix: "RED",
  source: "standard",
  stages: [
    {
      name: "升温",
      kind: "HEATING",
      gas: [0, 5, 0, 0],
      target_temp: 900,
      tolerance: 5,
      duration_min: 0,
      ramp_rate: 10,
      description: "升温",
    },
    {
      name: "还原",
      kind: "REDUCING",
      gas: [0, 10.5, 0, 4.5],
      target_temp: 900,
      tolerance: 5,
      duration_min: 180,
      ramp_rate: 0,
      description: "还原",
    },
  ],
};

describe("program utilities", () => {
  it("summarizes duration and gas in operator-facing Chinese", () => {
    expect(durationSummary(program)).toContain("180 min");
    expect(durationSummary(program)).toContain("动态阶段");
    expect(gasSummary(program.stages[1]!.gas)).toBe("10.5N₂ + 4.5CO");
  });

  it("copies a standard program into a custom draft", () => {
    const draft = copyAsCustom(program);
    expect(draft.id).toBe("");
    expect(draft.source).toBe("custom");
    expect(draft.name).toContain("自定义_");
    expect(draft.sample_prefix).toBe("RED");
    expect(draft.stages[0]).not.toBe(program.stages[0]);
  });

  it("normalizes sample prefixes for generated sample ids", () => {
    expect(normalizeSamplePrefix("rdi-01", "LOW_TEMP_DEGRADATION")).toBe(
      "RDI01",
    );
    expect(normalizeSamplePrefix("", "LOW_TEMP_DEGRADATION")).toBe("RDI");
  });

  it("blocks excessive flammable setpoints before saving", () => {
    const warning = validateProgramDraft(
      {
        ...program,
        source: "custom",
        category: "custom",
        stages: [{ ...program.stages[1]!, gas: [6, 10, 0, 7] }],
      },
      { h2: 5, co: 5, flow_steps: [0.01, 0.1, 0.01, 0.01] },
    );
    expect(warning.warnings).toHaveLength(0);
    expect(warning.errors.join(" ")).toContain("H₂");
    expect(warning.errors.join(" ")).toContain("CO");

    const invalid = validateProgramDraft({
      ...program,
      name: "",
      sample_prefix: "X",
      stages: [],
    });
    expect(invalid.errors.length).toBeGreaterThan(0);
  });

  it("dry-run preview explains normal and interrupted endings", () => {
    const steps = simulateProgram(program);
    expect(steps.at(-1)?.stage).toBe("完成");
    expect(steps.at(-1)?.condition).toContain("停止或故障");
    expect(steps.at(-1)?.atmosphere).toBe("10.5N₂ + 4.5CO");
  });
  it("rejects old duration combinations that would acquire different semantics", () => {
    for (const [stageIndex, duration] of [
      [0, 10],
      [1, 0],
    ]) {
      const legacy = structuredClone(program);
      legacy.stages[stageIndex!]!.duration_min = duration!;
      expect(validateProgramDraft(legacy).errors.join(" ")).toContain(
        "类型与时长不匹配",
      );
    }
  });
});
