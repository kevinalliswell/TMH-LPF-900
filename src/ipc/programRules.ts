import type { StageDto } from "./types";

/** Mirror the native engine's explicit timed/dynamic stage contract. */
export function validStageDuration(stage: StageDto): boolean {
  if (!Number.isFinite(stage.duration_min) || stage.duration_min < 0)
    return false;
  return ["STABILIZING", "REDUCING", "PURGING"].includes(stage.kind)
    ? stage.duration_min > 0
    : stage.duration_min === 0;
}
