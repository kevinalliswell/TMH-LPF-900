import { copyFile, readFile, readdir } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readSourceContext, validateSourceSbom, writeAssetInventory } from "./asset-inventory.mjs";

export function validateReleaseAssets({ tag, version, release, latest, names, signatures }) {
  if (release.isDraft !== true || release.tagName !== tag || tag !== `v${version}` || latest.version !== version) {
    throw new Error("Only the matching draft release may receive evidence");
  }
  if (!names.some((name) => name.endsWith(".dmg"))) throw new Error("Missing macOS disk image");
  for (const key of ["windows-x86_64", "darwin-aarch64", "darwin-x86_64"]) {
    const entry = latest.platforms?.[key];
    if (!entry?.signature?.trim()) throw new Error(`Missing updater entry: ${key}`);
    const url = new URL(entry.url);
    const name = decodeURIComponent(basename(url.pathname));
    if (url.protocol !== "https:" || url.username || url.password || !names.includes(name) ||
        !(key.startsWith("windows") ? name.endsWith(".exe") : name.endsWith(".tar.gz")) ||
        signatures.get(`${name}.sig`)?.trim() !== entry.signature.trim()) {
      throw new Error(`Updater asset/signature mismatch: ${key}`);
    }
  }
}

async function main() {
  const [directory, sbomPath, releasePath, tag] = process.argv.slice(2);
  if (!directory || !sbomPath || !releasePath || !tag) throw new Error("Usage: package-release.mjs ASSET_DIR SOURCE_SBOM RELEASE_METADATA TAG");
  const { commit, version } = await readSourceContext();
  validateSourceSbom(JSON.parse(await readFile(sbomPath, "utf8")), { commit, version });
  const entries = await readdir(directory, { withFileTypes: true });
  if (entries.some((entry) => !entry.isFile())) throw new Error("Release assets must be regular files");
  const names = entries.map(({ name }) => name);
  const signatures = new Map();
  for (const name of names.filter((name) => name.endsWith(".sig"))) signatures.set(name, await readFile(join(directory, name), "utf8"));
  const release = JSON.parse(await readFile(releasePath, "utf8"));
  validateReleaseAssets({ tag, version, release, names, signatures, latest: JSON.parse(await readFile(join(directory, "latest.json"), "utf8")) });
  await copyFile(sbomPath, join(directory, "source-sbom.cdx.json"));
  for (const source of ["CHANGELOG.md", "docs/09-发布与部署.md", "docs/12-版本治理路线.md", "docs/26-Issue推进与验收索引.md", "docs/27-Rust依赖安全基线.md", "docs/28-SBOM与交付资产校验.md"]) {
    await copyFile(source, join(directory, basename(source)));
  }
  const manifest = await writeAssetInventory(directory, {
    channel: "release_draft", tag, source_commit: commit, source_version: version,
    release_url: release.url, created_at: new Date().toISOString(),
    sbom_scope: "source lockfile inventory; all targets including development/build dependencies, not binary composition",
    signing_status: {
      updater: "signature files agree with latest.json; cryptographic/field update verification remains required",
      windows_publisher: "not verified by this inventory step",
      apple_notarization: "not verified by this inventory step",
    },
    limitations: ["No hardware, offline deployment, upgrade or rollback qualification is implied", "Draft/private asset URLs do not prove anonymous production distribution"],
  });
  console.log(JSON.stringify(manifest, null, 2));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
