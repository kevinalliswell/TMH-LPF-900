import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile, copyFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { execFileSync, spawnSync } from "node:child_process";

test("version sync replaces complete prereleases and fails on missing markers", async () => {
  const root = await mkdtemp(join(tmpdir(), "tmh-version-"));
  try {
    for (const dir of ["scripts", "src", "src/ipc", "src-tauri"]) await mkdir(join(root, dir), { recursive: true });
    await copyFile(new URL("./sync-version.mjs", import.meta.url), join(root, "scripts/sync-version.mjs"));
    for (const [path, content] of Object.entries({
      "package.json": JSON.stringify({ version: "1.2.3-rc.2+build.1" }),
      "Cargo.toml": '[workspace.package]\nversion = "0.2.0-rc.1"\n',
      "src-tauri/tauri.conf.json": JSON.stringify({ version: "0.2.0-rc.1", app: { windows: [{ title: "old" }] } }),
      "index.html": "<title>TMH-LPF-900 v0.2.0-rc.1</title>",
      "src/App.tsx": "TMH-LPF-900 v0.2.0-rc.1",
      "src/ipc/index.ts": 'const APP_VERSION = "0.2.0-rc.1";',
    })) await writeFile(join(root, path), content);
    const sync = (...args) => execFileSync(process.execPath, [join(root, "scripts/sync-version.mjs"), ...args], { stdio: "pipe" });
    sync();
    assert.equal(await readFile(join(root, "src/App.tsx"), "utf8"), "TMH-LPF-900 v1.2.3-rc.2+build.1");
    assert.equal(await readFile(join(root, "src/ipc/index.ts"), "utf8"), 'const APP_VERSION = "1.2.3-rc.2+build.1";');
    sync("--check");
    await writeFile(join(root, "src/ipc/index.ts"), "// marker removed");
    const failed = spawnSync(process.execPath, [join(root, "scripts/sync-version.mjs"), "--check"], { encoding: "utf8" });
    assert.notEqual(failed.status, 0);
    assert.match(failed.stderr, /marker/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
