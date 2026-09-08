import {
  mkdir,
  readdir,
  readFile,
  copyFile,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
const [bundleDir, platform] = process.argv.slice(2);
if (!bundleDir || !["windows-x64", "macos-universal"].includes(platform))
  throw new Error("Usage: package-candidate.mjs BUNDLE_DIR PLATFORM");
const suffix = platform === "windows-x64" ? ".exe" : ".dmg";
const sourceDir = path.join(
  bundleDir,
  platform === "windows-x64" ? "nsis" : "dmg",
);
const files = (await readdir(sourceDir))
  .filter((name) => name.endsWith(suffix))
  .sort();
if (files.length !== 1)
  throw new Error(`Expected one ${suffix} installer, found ${files.length}`);
const commit = execFileSync("git", ["rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
const version = JSON.parse(await readFile("package.json", "utf8")).version;
const output = path.join("candidate-assets", platform);
await mkdir(output, { recursive: true });
const name = `TMH-LPF-900-candidate-${commit.slice(0, 12)}-${platform}${suffix}`;
const bytes = await readFile(path.join(sourceDir, files[0]));
const sha256 = createHash("sha256").update(bytes).digest("hex");
await copyFile(path.join(sourceDir, files[0]), path.join(output, name));
const manifest = {
  channel: "candidate",
  source_commit: commit,
  source_version: version,
  platform,
  device_mode: "mock_only",
  updater_enabled: false,
  data_folder: "tmh-lpf-900-candidate",
  created_at: new Date().toISOString(),
  files: [{ name, bytes: bytes.length, sha256 }],
  validation:
    platform === "windows-x64"
      ? "NSIS install, first window, normal close, fresh database and uninstall smoke"
      : "Universal Mach-O and ad-hoc code signature, DMG verification",
  limitations: [
    "No real device or national-standard experiment validation",
    "No publisher certificate or Apple notarization",
    "Native manual acceptance remains required",
  ],
};
await writeFile(
  path.join(output, "manifest.json"),
  JSON.stringify(manifest, null, 2) + "\n",
);
await writeFile(path.join(output, "SHA256SUMS.txt"), `${sha256}  ${name}\n`);
await copyFile(
  "docs/20-候选安装包验收.md",
  path.join(output, "README-候选安装包.md"),
);
console.log(JSON.stringify(manifest, null, 2));
