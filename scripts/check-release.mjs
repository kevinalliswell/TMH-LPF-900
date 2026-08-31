#!/usr/bin/env node
import { readFile } from "node:fs/promises";

const tag = process.argv[2] ?? "";
const match = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/.exec(tag);
if (!match) {
  fail(`release tag must be SemVer with a v prefix; received ${JSON.stringify(tag)}`);
}

const version = tag.slice(1);
const [pkg, cargo, tauri, changelog] = await Promise.all([
  readJson("package.json"),
  readFile("Cargo.toml", "utf8"),
  readJson("src-tauri/tauri.conf.json"),
  readFile("CHANGELOG.md", "utf8"),
]);

const cargoVersion = cargo.match(/\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const checks = [
  ["package.json", pkg.version],
  ["Cargo.toml workspace.package", cargoVersion],
  ["src-tauri/tauri.conf.json", tauri.version],
];

for (const [source, actual] of checks) {
  if (actual !== version) {
    fail(`${source} version ${JSON.stringify(actual)} does not match ${tag}`);
  }
}

if (!changelog.includes(`## [${version}]`)) {
  fail(`CHANGELOG.md has no release section for ${version}`);
}

console.log(`release metadata matches ${tag}`);

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

function fail(message) {
  console.error(`release validation error: ${message}`);
  process.exit(1);
}
