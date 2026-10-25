import { mkdir, readdir, readFile, copyFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readSourceContext, validateSourceSbom, writeAssetInventory } from "./asset-inventory.mjs";

export async function packageCandidate({ root = process.cwd(), bundleDir, platform, sbomPath }) {
  if (!bundleDir || !sbomPath || !["windows-x64", "macos-universal"].includes(platform)) {
    throw new Error("Usage: package-candidate.mjs BUNDLE_DIR PLATFORM SOURCE_SBOM");
  }
  const suffix = platform === "windows-x64" ? ".exe" : ".dmg";
  const sourceDir = path.resolve(root, bundleDir, platform === "windows-x64" ? "nsis" : "dmg");
  const files = (await readdir(sourceDir)).filter((name) => name.endsWith(suffix)).sort();
  if (files.length !== 1) throw new Error(`Expected one ${suffix} installer, found ${files.length}`);
  const { commit, version } = await readSourceContext(root);
  sbomPath = path.resolve(root, sbomPath);
  validateSourceSbom(JSON.parse(await readFile(sbomPath, "utf8")), { commit, version });
  const output = path.join(root, "candidate-assets", platform);
  await mkdir(output, { recursive: true });
  if ((await readdir(output)).length) throw new Error("Candidate output must be empty; do not mix evidence from prior builds");
  const name = `TMH-LPF-900-candidate-${commit.slice(0, 12)}-${platform}${suffix}`;
  await copyFile(path.join(sourceDir, files[0]), path.join(output, name));
  await copyFile(sbomPath, path.join(output, "source-sbom.cdx.json"));
  await copyFile(path.join(root, "docs/20-候选安装包验收.md"), path.join(output, "README-候选安装包.md"));
  await copyFile(path.join(root, "docs/23-2026-09-13桌面验收记录.md"), path.join(output, "23-2026-09-13桌面验收记录.md"));
  await copyFile(path.join(root, "docs/28-SBOM与交付资产校验.md"), path.join(output, "28-SBOM与交付资产校验.md"));
  return writeAssetInventory(output, {
    channel: "candidate", source_commit: commit, source_version: version, platform,
    device_mode: "mock_only", updater_enabled: false, data_folder: "tmh-lpf-900-candidate",
    created_at: new Date().toISOString(),
    validation: platform === "windows-x64"
      ? "NSIS install, exact main window, normal close, fresh database, uninstall and separate production process smoke"
      : "Universal Mach-O and ad-hoc code signature, DMG verification",
    limitations: [
      "Source SBOM includes all lockfile targets and development/build dependencies; not a binary component inventory",
      "No real device or national-standard experiment validation",
      "No publisher certificate or Apple notarization",
      "Target-machine and actual-device acceptance remain required; local native mock evidence is documented separately",
    ],
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [bundleDir, platform, sbomPath] = process.argv.slice(2);
  packageCandidate({ bundleDir, platform, sbomPath }).then((manifest) => console.log(JSON.stringify(manifest, null, 2)))
    .catch((error) => { console.error(error.message); process.exitCode = 1; });
}
