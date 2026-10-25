import { createReadStream } from "node:fs";
import { readdir, writeFile, readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { join } from "node:path";

export async function readSourceContext(root = process.cwd()) {
  const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
  if (git("status", "--porcelain")) throw new Error("Asset packaging requires a clean checkout");
  return { commit: git("rev-parse", "HEAD"), version: JSON.parse(await readFile(join(root, "package.json"), "utf8")).version };
}

export function validateSourceSbom(sbom, { commit, version }) {
  const properties = new Map((sbom.metadata?.properties ?? []).map(({ name, value }) => [name, value]));
  if (sbom.bomFormat !== "CycloneDX" || sbom.metadata?.component?.version !== version ||
      properties.get("tmh:source_commit") !== commit || properties.get("tmh:source_version") !== version ||
      properties.get("tmh:working_tree_dirty") !== "false") {
    throw new Error("SBOM source/version must match this clean checkout");
  }
}

async function fingerprint(directory, name) {
  const hash = createHash("sha256");
  let bytes = 0;
  for await (const chunk of createReadStream(join(directory, name))) {
    hash.update(chunk);
    bytes += chunk.length;
  }
  return { name, bytes, sha256: hash.digest("hex") };
}

// The manifest lists payloads; the checksum file also covers the manifest itself.
// Neither file tries to contain its own digest.
export async function writeAssetInventory(directory, metadata) {
  const entries = await readdir(directory, { withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isFile() || /[\\\r\n]/.test(entry.name)) throw new Error(`Unsafe asset entry: ${entry.name}`);
  }
  const names = entries.map(({ name }) => name).filter((name) => !["manifest.json", "SHA256SUMS.txt"].includes(name)).sort();
  if (!names.length) throw new Error("No release payloads found");
  const files = [];
  for (const name of names) files.push(await fingerprint(directory, name));
  const manifest = { ...metadata, files };
  await writeFile(join(directory, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  const manifestFile = await fingerprint(directory, "manifest.json");
  await writeFile(join(directory, "SHA256SUMS.txt"), [...files, manifestFile]
    .sort((a, b) => a.name.localeCompare(b.name, "en"))
    .map(({ name, sha256 }) => `${sha256}  ${name}\n`).join(""));
  return manifest;
}
