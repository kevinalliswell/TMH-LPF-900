import { mkdir, readFile, writeFile } from "node:fs/promises";
import { execFileSync, spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function parseScan(scan) {
  if (scan.error || ![0, 1].includes(scan.status)) throw scan.error ?? new Error(`cargo-audit failed: ${scan.status ?? scan.signal}`);
  // cargo-audit can log registry lookup errors without changing its advisory exit
  // status. Do not let the known glib finding conceal an incomplete yanked check.
  if (scan.stderr?.trim()) throw new Error("cargo-audit emitted diagnostics; inspect rust-audit.stderr.log");
  return JSON.parse(scan.stdout);
}

// Preserve the unfiltered scanner report; exceptions affect the gate decision only.
export function evaluateAudit(report, exceptions, now = new Date()) {
  const settings = report?.settings;
  if (!settings || ["ignore", "target_arch", "target_os"].some((key) => !Array.isArray(settings[key]) || settings[key].length) ||
      settings.severity !== null || !Array.isArray(settings.informational_warnings) ||
      ["unmaintained", "unsound", "notice"].some((kind) => !settings.informational_warnings.includes(kind))) {
    throw new Error("Missing or filtered RustSec scan");
  }
  const vulnerabilities = report.vulnerabilities;
  if (!(report.database?.["advisory-count"] > 0) || !(report.lockfile?.["dependency-count"] > 0) ||
      !Array.isArray(vulnerabilities?.list) || vulnerabilities.count !== vulnerabilities.list.length ||
      vulnerabilities.found !== (vulnerabilities.list.length > 0) || !report.warnings ||
      Object.values(report.warnings).some((entries) => !Array.isArray(entries))) {
    throw new Error("Incomplete RustSec report");
  }
  const date = (value) => {
    if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) throw new Error("Invalid review date");
    const parsed = Date.parse(`${value}T00:00:00Z`);
    if (!Number.isFinite(parsed) || new Date(parsed).toISOString().slice(0, 10) !== value) throw new Error("Invalid review date");
    return parsed;
  };
  for (const item of exceptions) {
    const reviewed = date(item.reviewed_at);
    const expires = date(item.expires);
    if (expires <= now.getTime()) throw new Error(`Audit exception expired: ${item.id}`);
    if (reviewed > now.getTime() || expires <= reviewed || expires - reviewed > 30 * 86400000 ||
        ["id", "package", "version", "reason", "owner"].some((key) => typeof item[key] !== "string" || !item[key].trim())) {
      throw new Error("Invalid audit exception: require exact package/version, owner, reason and at most thirty days");
    }
  }
  const accepted = [];
  const denied = [];
  for (const entry of [...vulnerabilities.list, ...(report.warnings.unsound ?? [])]) {
    const exception = exceptions.find((item) => item.id === entry.advisory?.id &&
      item.package === entry.package?.name && item.version === entry.package?.version);
    if (exception) accepted.push(exception);
    else denied.push(`${entry.advisory?.id}: ${entry.package?.name} ${entry.package?.version}`);
  }
  if (denied.length) throw new Error(`Unaccepted RustSec findings:\n${denied.join("\n")}`);
  return { accepted, warning_count: Object.values(report.warnings).reduce((count, entries) => count + entries.length, 0) };
}

async function main() {
  const database = process.argv[2];
  if (!database) throw new Error("Usage: node scripts/audit-rust.mjs RUSTSEC_DATABASE_PATH (fresh official Git checkout)");
  await mkdir("verification", { recursive: true });
  const commit = execFileSync("git", ["-C", database, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  await writeFile("verification/rustsec-commit.txt", `${commit}\n`);
  const scan = spawnSync("cargo", ["audit", "--file", "Cargo.lock", "--db", database, "--no-fetch", "--json", "--deny", "unsound"], { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  await writeFile("verification/rust-audit.json", scan.stdout ?? "");
  await writeFile("verification/rust-audit.stderr.log", scan.stderr ?? "");
  if (scan.stderr) process.stderr.write(scan.stderr);
  const report = parseScan(scan);
  const exceptions = JSON.parse(await readFile("config/rust-audit-exceptions.json", "utf8"));
  const decision = evaluateAudit(report, exceptions);
  if (scan.status === 1 && !report.vulnerabilities.list.length && !(report.warnings.unsound?.length)) {
    throw new Error("cargo-audit failed without a complete advisory result");
  }
  await writeFile("verification/rust-audit-decision.json", JSON.stringify(decision, null, 2) + "\n");
  console.log(JSON.stringify(decision, null, 2));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
