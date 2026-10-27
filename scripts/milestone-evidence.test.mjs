import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { missingMilestoneEvidence } from "./milestone-evidence.mjs";

test("every checked milestone item cites its acceptance evidence", async () => {
  const source = await readFile(new URL("../docs/10-里程碑与验收标准.md", import.meta.url), "utf8");
  assert.deepEqual(missingMilestoneEvidence(source), []);
});

test("the milestone evidence gate rejects a completed item without a reference", () => {
  for (const item of [
    "- [x] Mock IPC authorization matrix passed",
    "- [x] Mock IPC authorization matrix passed（验收证据： ）",
  ]) assert.deepEqual(missingMilestoneEvidence(item), [1]);
});

test("the milestone evidence gate accepts evidence and ignores unchecked items", () => {
  const source = [
    "- [x] Mock IPC authorization matrix passed（验收证据：PR #125，protected_ipc_gap_tests）",
    "- [ ] Verify the real device against the approved SOP",
  ].join("\n");
  assert.deepEqual(missingMilestoneEvidence(source), []);
});
