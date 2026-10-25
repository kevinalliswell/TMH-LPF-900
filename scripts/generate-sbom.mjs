import { mkdtemp, copyFile, readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function validateLockCoverage(sbom, cargoLock, packageJson) {
  if (sbom.bomFormat !== "CycloneDX" || sbom.specVersion !== "1.7" ||
      !sbom.metadata?.tools?.components?.some((tool) => tool.name === "syft" && tool.version === "1.55.0")) {
    throw new Error("Expected CycloneDX 1.7 from pinned Syft 1.55.0");
  }
  const components = sbom.components ?? [];
  const missing = [];
  const packages = cargoLock.split("[[package]]").slice(1);
  if (!packages.length) throw new Error("Cargo.lock contains no packages");
  for (const block of packages) {
    const name = block.match(/^name = "([^"]+)"/m)?.[1];
    const version = block.match(/^version = "([^"]+)"/m)?.[1];
    if (!name || !version || !components.some((item) => item.name === name && item.version === version)) missing.push(`${name}@${version}`);
  }
  for (const name of Object.keys(packageJson.dependencies)) {
    if (!components.some((item) => item.name === name && item.version && item.purl?.startsWith("pkg:npm/"))) missing.push(name);
  }
  if (missing.length) throw new Error(`Incomplete lockfile SBOM: ${missing.join(", ")}`);
}

async function main() {
  const [tool, destination = "verification/source-sbom.cdx.json"] = process.argv.slice(2);
  if (!tool) throw new Error("Usage: generate-sbom.mjs SYFT_BINARY [OUTPUT]");
  const runGit = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
  const commit = runGit("rev-parse", "HEAD");
  const dirty = runGit("status", "--porcelain") !== "";
  const inputs = ["Cargo.lock", "pnpm-lock.yaml", "Cargo.toml", "package.json"];
  const staging = await mkdtemp(join(tmpdir(), "tmh-sbom-"));
  const output = resolve(destination);
  try {
    for (const name of inputs) await copyFile(name, join(staging, name));
    const pkg = JSON.parse(await readFile(join(staging, "package.json"), "utf8"));
    await mkdir(dirname(output), { recursive: true });
    execFileSync(resolve(tool), ["scan", `dir:${staging}`, "--source-name", "TMH-LPF-900 source lockfiles", "--source-version", pkg.version,
      "--output", `cyclonedx-json=${output}`], { stdio: "inherit", env: { ...process.env, SYFT_CHECK_FOR_APP_UPDATE: "false" } });
    const sbom = JSON.parse(await readFile(output, "utf8"));
    validateLockCoverage(sbom, await readFile(join(staging, "Cargo.lock"), "utf8"), pkg);
    const properties = [
      { name: "tmh:source_commit", value: commit },
      { name: "tmh:source_version", value: pkg.version },
      { name: "tmh:working_tree_dirty", value: String(dirty) },
      { name: "tmh:scope", value: "source lockfile inventory; all targets, including development/build dependencies; not a binary SBOM" },
    ];
    for (const name of inputs) properties.push({ name: `tmh:input_sha256:${name}`, value: createHash("sha256").update(await readFile(join(staging, name))).digest("hex") });
    sbom.metadata.properties = properties;
    await writeFile(output, JSON.stringify(sbom, null, 2) + "\n");
    console.log(`Source SBOM: ${sbom.components.length} components; commit ${commit}; dirty=${dirty}`);
  } finally { await rm(staging, { recursive: true }); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
