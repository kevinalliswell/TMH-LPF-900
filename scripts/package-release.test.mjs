import { test } from "node:test";
import assert from "node:assert/strict";
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { validateReleaseAssets } from "./package-release.mjs";

const fixture = () => {
  const latest = { version: "0.2.0", platforms: {} };
  const signatures = new Map();
  for (const [key, name] of [["windows-x86_64", "TMH_0.2.0_x64-setup.exe"], ["darwin-aarch64", "TMH_universal.app.tar.gz"], ["darwin-x86_64", "TMH_universal.app.tar.gz"]]) {
    latest.platforms[key] = { url: `https://github.com/example/repo/releases/download/v0.2.0/${name}`, signature: "test-signature" };
    signatures.set(`${name}.sig`, "test-signature\n");
  }
  return { tag: "v0.2.0", version: "0.2.0", release: { isDraft: true, tagName: "v0.2.0" }, latest,
    names: ["TMH_0.2.0_x64-setup.exe", "TMH_universal.app.tar.gz", "TMH_universal.dmg", ...signatures.keys()], signatures };
};
test("draft release requires both universal updater entries and matching signature files", () => {
  assert.doesNotThrow(() => validateReleaseAssets(fixture()));
  for (const change of [
    (x) => { x.release.isDraft = false; },
    (x) => { x.release.tagName = "v0.1.0"; },
    (x) => { x.latest.version = "0.1.0"; },
    (x) => { delete x.latest.platforms["darwin-x86_64"]; },
    (x) => { x.names = x.names.filter((name) => !name.endsWith(".dmg")); },
    (x) => { x.signatures.delete("TMH_0.2.0_x64-setup.exe.sig"); },
    (x) => { x.latest.platforms["windows-x86_64"].signature = "different"; },
    (x) => { x.latest.platforms["windows-x86_64"].url = "https://example.invalid/missing.exe"; },
    (x) => { x.latest.platforms["windows-x86_64"].url = "http://example.invalid/TMH_0.2.0_x64-setup.exe"; },
  ]) {
    const input = fixture(); change(input);
    assert.throws(() => validateReleaseAssets(input));
  }
});

const releaseScript = fileURLToPath(new URL("./package-release.mjs", import.meta.url));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const sourceFiles = ["CHANGELOG.md", "docs/09-发布与部署.md", "docs/12-版本治理路线.md", "docs/26-Issue推进与验收索引.md", "docs/27-Rust依赖安全基线.md", "docs/28-SBOM与交付资产校验.md"];

async function withReleaseFixture(run) {
  const root = await mkdtemp(join(tmpdir(), "tmh-release-inventory-"));
  const git = (...args) => execFileSync("git", ["-c", "user.name=Release Inventory Test", "-c", "user.email=inventory@example.invalid", ...args], { cwd: root, encoding: "utf8", stdio: "pipe" }).trim();
  try {
    await mkdir(join(root, "docs"));
    await writeFile(join(root, ".gitignore"), "release-assets/\nverification/\n");
    await writeFile(join(root, "package.json"), JSON.stringify({ version: "0.2.0" }) + "\n");
    for (const source of sourceFiles) await writeFile(join(root, source), `# ${source}\n离线fixture，不代表真实发布或现场验收。\n`);
    git("init", "-b", "main"); git("add", "."); git("commit", "-m", "release inventory fixture");
    const commit = git("rev-parse", "HEAD");
    const directory = join(root, "release-assets");
    const evidence = join(root, "verification");
    await mkdir(directory); await mkdir(evidence);
    const input = fixture();
    for (const name of input.names) {
      const bytes = input.signatures.has(name) ? input.signatures.get(name) : Buffer.from([0x4d, 0x5a, 0x00, 0xff, 0x80]);
      await writeFile(join(directory, name), bytes);
    }
    await writeFile(join(directory, "latest.json"), JSON.stringify(input.latest, null, 2) + "\n");
    const release = { ...input.release, url: "https://github.com/example/repo/releases/tag/v0.2.0" };
    const releasePath = join(evidence, "draft.json");
    await writeFile(releasePath, JSON.stringify(release));
    const sbomPath = join(evidence, "source.cdx.json");
    await writeFile(sbomPath, JSON.stringify({
      bomFormat: "CycloneDX", specVersion: "1.7", version: 1,
      metadata: { component: { type: "application", name: "TMH-LPF-900", version: "0.2.0" }, properties: [
        { name: "tmh:source_commit", value: commit },
        { name: "tmh:source_version", value: "0.2.0" },
        { name: "tmh:working_tree_dirty", value: "false" },
      ] }, components: [],
    }, null, 2) + "\n");
    assert.equal(git("status", "--porcelain"), "");
    const args = [releaseScript, directory, sbomPath, releasePath, input.tag];
    await run({ root, git, commit, directory, sbomPath, releasePath, release, names: input.names, args });
  } finally { await rm(root, { recursive: true, force: true }); }
}

test("offline release CLI inventories complete fake draft assets, evidence and manifest without publishing", async () => {
  await withReleaseFixture(async (input) => {
    const stdout = execFileSync(process.execPath, input.args, { cwd: input.root, encoding: "utf8", stdio: "pipe" });
    const manifest = JSON.parse(stdout);
    assert.deepEqual(JSON.parse(await readFile(join(input.directory, "manifest.json"), "utf8")), manifest);
    assert.equal(manifest.channel, "release_draft");
    assert.equal(manifest.tag, "v0.2.0");
    assert.equal(manifest.source_commit, input.commit);
    assert.equal(manifest.source_version, "0.2.0");
    assert.equal(manifest.release_url, input.release.url);
    assert.match(manifest.sbom_scope, /not binary/);
    assert.match(manifest.signing_status.updater, /cryptographic.*remains required/);
    const payloadNames = [...input.names, "latest.json", "source-sbom.cdx.json", ...sourceFiles.map((source) => basename(source))].sort();
    assert.deepEqual(manifest.files.map((file) => file.name).sort(), payloadNames);
    for (const file of manifest.files) {
      const bytes = await readFile(join(input.directory, file.name));
      assert.equal(file.bytes, bytes.length, file.name);
      assert.equal(file.sha256, digest(bytes), file.name);
    }
    assert.deepEqual(await readFile(join(input.directory, "source-sbom.cdx.json")), await readFile(input.sbomPath));
    for (const source of sourceFiles) assert.deepEqual(await readFile(join(input.directory, basename(source))), await readFile(join(input.root, source)));
    const lines = (await readFile(join(input.directory, "SHA256SUMS.txt"), "utf8")).trim().split("\n");
    const sums = new Map(lines.map((line) => {
      const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
      assert.ok(match, `invalid checksum row: ${line}`);
      return [match[2], match[1]];
    }));
    assert.equal(sums.size, lines.length);
    assert.deepEqual([...sums.keys()].sort(), [...payloadNames, "manifest.json"].sort());
    for (const [name, hash] of sums) assert.equal(hash, digest(await readFile(join(input.directory, name))), name);
    assert.deepEqual((await readdir(input.directory)).sort(), [...payloadNames, "manifest.json", "SHA256SUMS.txt"].sort());
    assert.equal(input.git("status", "--porcelain"), "");
    assert.equal(input.git("tag", "--list"), "", "inventory must not create a real Git tag");
  });
});

test("offline release CLI rejects changed checkout despite a previously clean source SBOM", async () => {
  await withReleaseFixture(async (input) => {
    await writeFile(join(input.root, "CHANGELOG.md"), "modified after clean source SBOM generation\n");
    const result = spawnSync(process.execPath, input.args, { cwd: input.root, encoding: "utf8" });
    assert.equal(result.status, 1, "a clean SBOM cannot certify later tracked source edits");
    await assert.rejects(access(join(input.directory, "manifest.json")), { code: "ENOENT" });
    assert.equal(input.git("tag", "--list"), "");
  });
});
