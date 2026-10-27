import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { releaseVersion, isPrerelease, validateReleaseMetadata, verifyReleaseRef } from "./check-release.mjs";

test("SemVer tags reject empty and leading-zero prerelease identifiers", () => {
  for (const tag of ["v1.2.3", "v0.2.0-rc.1", "v1.2.3+build.01", "v1.2.3-0.a-1"]) {
    assert.equal(releaseVersion(tag), tag.slice(1));
  }
  for (const tag of ["", "1.2.3", "v01.2.3", "v1.2.3-01", "v1.2.3-rc..1", "v1.2.3-.", "v1.2.3+", "v1.2.3+build..1", "v1.2.3\n"]) {
    assert.throws(() => releaseVersion(tag), /SemVer/);
  }
});
const metadata = () => ({ packageVersion: "0.2.0", cargo: '[workspace.package]\nversion = "0.2.0"\n', tauriVersion: "0.2.0", changelog: "## [0.2.0] - 2026-08-31\n" });
test("all version points and an exact changelog heading are required", () => {
  assert.doesNotThrow(() => validateReleaseMetadata("v0.2.0", metadata()));
  for (const edit of [
    { packageVersion: "0.1.0" }, { tauriVersion: "0.1.0" },
    { cargo: '[package]\nversion = "0.2.0"' },
    { changelog: "example: ## [0.2.0]" }, { changelog: "## [0.2.0]extra" },
  ]) assert.throws(() => validateReleaseMetadata("v0.2.0", { ...metadata(), ...edit }));
});
test("release provenance requires an annotated tag at checkout and main tip", async () => {
  const cwd = await mkdtemp(join(tmpdir(), "tmh-release-"));
  const git = (...args) => execFileSync("git", ["-c", "user.name=Release Test", "-c", "user.email=release@example.invalid", ...args], { cwd, stdio: "pipe", encoding: "utf8" }).trim();
  try {
    git("init", "-b", "main");
    await writeFile(join(cwd, "file"), "baseline");
    git("add", "file"); git("commit", "-m", "baseline");
    git("update-ref", "refs/remotes/origin/main", "HEAD");
    git("tag", "v0.2.0");
    assert.throws(() => verifyReleaseRef("v0.2.0", cwd), /annotated/);
    git("tag", "-d", "v0.2.0"); git("tag", "-a", "v0.2.0", "-m", "release");
    assert.doesNotThrow(() => verifyReleaseRef("v0.2.0", cwd));
    await writeFile(join(cwd, "file"), "side-branch work");
    git("commit", "-am", "later work");
    assert.throws(() => verifyReleaseRef("v0.2.0", cwd), /checkout/);
    git("tag", "-a", "v0.2.1", "-m", "side branch");
    assert.throws(() => verifyReleaseRef("v0.2.1", cwd), /main tip/);
  } finally { await rm(cwd, { recursive: true, force: true }); }
});

test("build-metadata hyphens do not turn a release into a prerelease", () => {
  assert.equal(isPrerelease("v1.2.3+release-build.1"), false);
  assert.equal(isPrerelease("v1.2.3-rc.1+release-build.1"), true);
});

test("updater metadata uses the public source repository releases endpoint", async () => {
  const config = JSON.parse(await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.deepEqual(config.plugins.updater.endpoints, [
    "https://github.com/kevinalliswell/TMH-LPF-900/releases/latest/download/latest.json",
  ]);
});
