// Source and runtime context only. Never collect credentials, home paths or field data.
import { execFileSync } from "node:child_process";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
const output = process.argv[2] ?? "verification/context.json";
const run = (cmd, args) => execFileSync(cmd, args, { encoding: "utf8", stdio: "pipe" }).trim();
const pkg = JSON.parse(await readFile("package.json", "utf8"));
const context = {
  source_commit: run("git", ["rev-parse", "HEAD"]),
  // cat-file preserves parent headers even when checkout fetch-depth=1 marks HEAD shallow.
  source_parents: run("git", ["cat-file", "-p", "HEAD"]).split("\n\n")[0]
    .split("\n").filter((line) => line.startsWith("parent ")).map((line) => line.slice(7)),
  working_tree_dirty: run("git", ["status", "--porcelain"]) !== "",
  version: pkg.version,
  recorded_at: new Date().toISOString(),
  platform: process.platform,
  arch: process.arch,
  node: process.version,
  rust: run("rustc", ["--version"]),
  package_manager: pkg.packageManager,
  package_manager_runtime: process.env.npm_config_user_agent ?? null,
  github_run_id: process.env.GITHUB_RUN_ID ?? null,
  github_event: process.env.GITHUB_EVENT_NAME ?? null,
  scope: "automated software verification; no hardware or field acceptance",
};
await mkdir(dirname(output), { recursive: true });
await writeFile(output, JSON.stringify(context, null, 2) + "\n");
console.log(`verification context saved to ${output}`);
