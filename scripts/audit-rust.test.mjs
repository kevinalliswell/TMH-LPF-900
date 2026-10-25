import { test } from "node:test";
import assert from "node:assert/strict";
import { evaluateAudit, parseScan } from "./audit-rust.mjs";

const finding = (id = "RUSTSEC-2024-0429", version = "0.18.5") => ({
  advisory: { id }, package: { name: "glib", version },
});
const report = () => ({
  database: { "advisory-count": 1352 }, lockfile: { "dependency-count": 500 },
  settings: { ignore: [], target_arch: [], target_os: [], severity: null, informational_warnings: ["unsound", "unmaintained", "notice"] },
  vulnerabilities: { found: false, count: 0, list: [] }, warnings: {},
});
const exceptions = () => [{
  id: "RUSTSEC-2024-0429", package: "glib", version: "0.18.5",
  reviewed_at: "2026-10-25", expires: "2026-11-20", owner: "kevinalliswell",
  reason: "Linux GTK3 dependency; see docs/27 for scope and unresolved limitations.",
}];
const now = new Date("2026-10-25T00:00:00Z");

test("audit fails on a new vulnerability even when an unrelated exception exists", () => {
  const input = report();
  input.vulnerabilities = { found: true, count: 1, list: [finding("RUSTSEC-2026-0346")] };
  assert.throws(() => evaluateAudit(input, exceptions(), now), /RUSTSEC-2026-0346/);
});
test("only the reviewed unsound package/version is temporarily accepted, and stays visible", () => {
  const input = report();
  input.warnings = { unsound: [finding()], unmaintained: [finding("RUSTSEC-2024-0370")] };
  assert.deepEqual(evaluateAudit(input, exceptions(), now), { accepted: exceptions(), warning_count: 2 });
  input.warnings.unsound = [finding(undefined, "0.18.6")];
  assert.throws(() => evaluateAudit(input, exceptions(), now), /0.18.6/);
  input.warnings.unsound = [finding("RUSTSEC-2026-0386")];
  assert.throws(() => evaluateAudit(input, exceptions(), now), /RUSTSEC-2026-0386/);
});
test("exceptions expire, cannot be future-dated, and cannot exceed thirty days", () => {
  const input = report();
  input.warnings.unsound = [finding()];
  assert.throws(() => evaluateAudit(input, exceptions(), new Date("2026-11-20T00:00:00Z")), /expired/);
  for (const edit of [{ reviewed_at: "2026-10-26" }, { expires: "2026-12-01" }, { owner: "" }, { expires: "2026-11-31" }]) {
    assert.throws(() => evaluateAudit(input, [{ ...exceptions()[0], ...edit }], now));
  }
});
test("empty, truncated or filtered scans cannot pass as a clean audit", () => {
  for (const input of [{}, { ...report(), database: null }, { ...report(), vulnerabilities: { found: false, count: 1, list: [] } }]) {
    assert.throws(() => evaluateAudit(input, [], now));
  }
  for (const edit of [{ ignore: ["RUSTSEC-2026-0346"] }, { target_os: ["windows"] }, { severity: "critical" }, { informational_warnings: [] }, { informational_warnings: ["unsound"] }]) {
    assert.throws(() => evaluateAudit({ ...report(), settings: { ...report().settings, ...edit } }, [], now), /filtered/);
  }
  assert.deepEqual(evaluateAudit(report(), [], now), { accepted: [], warning_count: 0 });
});

test("scanner errors and registry diagnostics cannot be hidden by accepted findings", () => {
  const stdout = JSON.stringify({ ...report(), warnings: { unsound: [finding()] } });
  assert.deepEqual(parseScan({ status: 1, stdout, stderr: "" }).warnings.unsound, [finding()]);
  for (const edit of [{ status: 2 }, { status: null, signal: "SIGTERM" }, { stderr: "couldn't check if the package is yanked" }, { stdout: "" }, { error: new Error("spawn failed") }]) {
    assert.throws(() => parseScan({ status: 1, stdout, stderr: "", ...edit }));
  }
});
