#!/usr/bin/env node
import { readFile, appendFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function releaseVersion(tag) {
  const match = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/.exec(tag);
  const identifiers = (value) => value === undefined ? [] : value.split(".");
  if (!match || match[0] !== tag || identifiers(match[4]).some((id) => !id || /^0\d+$/.test(id)) ||
      identifiers(match[5]).some((id) => !id)) {
    throw new Error(`release tag must be SemVer with a v prefix; received ${JSON.stringify(tag)}`);
  }
  return tag.slice(1);
}

export function isPrerelease(tag) {
  return releaseVersion(tag).split("+")[0].includes("-");
}

export function validateReleaseMetadata(tag, { packageVersion, cargo, tauriVersion, changelog }) {
  const version = releaseVersion(tag);
  const workspace = cargo.split("[workspace.package]")[1]?.split(/\n\[/)[0];
  const cargoVersion = workspace?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  for (const [source, actual] of [["package.json", packageVersion], ["Cargo.toml workspace.package", cargoVersion], ["tauri.conf.json", tauriVersion]]) {
    if (actual !== version) throw new Error(`${source} version ${JSON.stringify(actual)} does not match ${tag}`);
  }
  if (!changelog.split(/\r?\n/).some((line) => line === `## [${version}]` || line.startsWith(`## [${version}] - `))) {
    throw new Error(`CHANGELOG.md has no release section for ${version}`);
  }
}

export function verifyReleaseRef(tag, cwd = process.cwd()) {
  releaseVersion(tag);
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: "pipe" }).trim();
  const ref = `refs/tags/${tag}`;
  if (git("cat-file", "-t", ref) !== "tag") throw new Error("release requires an annotated tag");
  const commit = git("rev-parse", "--verify", `${ref}^{commit}`);
  if (commit !== git("rev-parse", "HEAD")) throw new Error("release tag must match checkout HEAD");
  if (commit !== git("rev-parse", "refs/remotes/origin/main")) throw new Error("release tag must point at the current main tip");
}

async function main() {
  const tag = process.argv[2] ?? "";
  const [pkg, cargo, tauri, changelog] = await Promise.all([
    readFile("package.json", "utf8"), readFile("Cargo.toml", "utf8"),
    readFile("src-tauri/tauri.conf.json", "utf8"), readFile("CHANGELOG.md", "utf8"),
  ]);
  validateReleaseMetadata(tag, { packageVersion: JSON.parse(pkg).version, cargo, tauriVersion: JSON.parse(tauri).version, changelog });
  verifyReleaseRef(tag);
  if (process.env.GITHUB_OUTPUT) await appendFile(process.env.GITHUB_OUTPUT, `prerelease=${isPrerelease(tag)}\n`);
  console.log(`release metadata and provenance match ${tag}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(`release validation error: ${error.message}`); process.exitCode = 1; });
}
