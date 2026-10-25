import { test } from "node:test";
import assert from "node:assert/strict";
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { packageCandidate } from "./package-candidate.mjs";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const sourceDocs = ["20-候选安装包验收.md", "23-2026-09-13桌面验收记录.md", "28-SBOM与交付资产校验.md"];
const copiedDocs = ["README-候选安装包.md", ...sourceDocs.slice(1)];

async function fixture(platform, run) {
  const root = await mkdtemp(join(tmpdir(), "tmh-candidate-"));
  const git = (...args) => execFileSync("git", ["-c", "user.name=Candidate Test", "-c", "user.email=candidate@example.invalid", ...args], { cwd: root, encoding: "utf8", stdio: "pipe" }).trim();
  try {
    await mkdir(join(root, "docs"));
    await mkdir(join(root, "verification"));
    await writeFile(join(root, ".gitignore"), "target/\ncandidate-assets/\nverification/\n");
    await writeFile(join(root, "package.json"), JSON.stringify({ version: "0.2.0" }) + "\n");
    for (const name of sourceDocs) await writeFile(join(root, "docs", name), `# ${name}\n模拟验收文档；不是现场证据。\n`);
    git("init", "-b", "main"); git("add", "."); git("commit", "-m", "candidate fixture");
    const commit = git("rev-parse", "HEAD");
    const bundleDir = join(root, "target", "bundle");
    const installerDir = join(bundleDir, platform === "windows-x64" ? "nsis" : "dmg");
    const suffix = platform === "windows-x64" ? ".exe" : ".dmg";
    await mkdir(installerDir, { recursive: true });
    const installer = join(installerDir, `mock-installer${suffix}`);
    await writeFile(installer, Buffer.from([0x4d, 0x5a, 0x00, 0x01, 0x80, 0xff]));
    const sbomPath = join(root, "verification", "source.cdx.json");
    const sbom = {
      bomFormat: "CycloneDX", specVersion: "1.6", version: 1,
      metadata: {
        component: { type: "application", name: "TMH-LPF-900", version: "0.2.0" },
        properties: [
          { name: "tmh:source_commit", value: commit },
          { name: "tmh:source_version", value: "0.2.0" },
          { name: "tmh:working_tree_dirty", value: "false" },
        ],
      },
      components: [],
    };
    await writeFile(sbomPath, JSON.stringify(sbom, null, 2) + "\n");
    assert.equal(git("status", "--porcelain"), "", "generated evidence stays outside the source tree");
    await run({ request: { root, bundleDir, platform, sbomPath }, root, platform, sbomPath, commit, sbom, installer, installerDir, suffix });
  } finally { await rm(root, { recursive: true, force: true }); }
}

for (const platform of ["windows-x64", "macos-universal"]) {
  test(`${platform} candidate binds source SBOM and hashes every delivered byte including manifest`, async () => {
    await fixture(platform, async (input) => {
      const manifest = await packageCandidate(input.request);
      const output = join(input.root, "candidate-assets", platform);
      const installerName = `TMH-LPF-900-candidate-${input.commit.slice(0, 12)}-${platform}${input.suffix}`;
      const payloadNames = [installerName, "source-sbom.cdx.json", ...copiedDocs].sort();
      assert.equal(manifest.source_commit, input.commit);
      assert.equal(manifest.source_version, "0.2.0");
      assert.equal(manifest.platform, platform);
      assert.equal(manifest.device_mode, "mock_only");
      assert.equal(manifest.updater_enabled, false);
      assert.deepEqual(manifest.files.map((file) => file.name).sort(), payloadNames);
      for (const entry of manifest.files) {
        const actual = await readFile(join(output, entry.name));
        assert.equal(entry.bytes, actual.length, entry.name);
        assert.equal(entry.sha256, sha256(actual), entry.name);
      }
      assert.deepEqual(await readFile(join(output, installerName)), await readFile(input.installer));
      assert.deepEqual(await readFile(join(output, "source-sbom.cdx.json")), await readFile(input.sbomPath));
      for (let i = 0; i < sourceDocs.length; i++) {
        assert.deepEqual(await readFile(join(output, copiedDocs[i])), await readFile(join(input.root, "docs", sourceDocs[i])));
      }
      assert.deepEqual(JSON.parse(await readFile(join(output, "manifest.json"), "utf8")), manifest);
      const checksumLines = (await readFile(join(output, "SHA256SUMS.txt"), "utf8")).trim().split("\n");
      const checksums = new Map(checksumLines.map((line) => {
        const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
        assert.ok(match, `invalid checksum row: ${line}`);
        return [match[2], match[1]];
      }));
      assert.equal(checksums.size, checksumLines.length, "no duplicate checksum names");
      assert.deepEqual([...checksums.keys()].sort(), [...payloadNames, "manifest.json"].sort());
      for (const [name, hash] of checksums) assert.equal(hash, sha256(await readFile(join(output, name))), name);
      assert.deepEqual((await readdir(output)).sort(), [...payloadNames, "manifest.json", "SHA256SUMS.txt"].sort());
    });
  });
}

test("candidate refuses missing or ambiguous installers without a passing manifest", async () => {
  for (const count of [0, 2]) {
    await fixture("windows-x64", async (input) => {
      if (count === 0) await rm(input.installer);
      else await writeFile(join(input.installerDir, "second.exe"), "another installer");
      await assert.rejects(() => packageCandidate(input.request));
      await assert.rejects(access(join(input.root, "candidate-assets", input.platform, "manifest.json")), { code: "ENOENT" });
    });
  }
});

test("candidate refuses mismatched, dirty or incomplete SBOM provenance without a passing manifest", async () => {
  const changes = [
    (bom) => { bom.metadata.properties[0].value = "0".repeat(40); },
    (bom) => { bom.metadata.properties[1].value = "0.3.0"; },
    (bom) => { bom.metadata.properties[2].value = "true"; },
    (bom) => { bom.metadata.properties[2].value = false; },
    (bom) => { bom.metadata.component.version = "0.3.0"; },
    (bom) => { bom.metadata.properties = []; },
    (bom) => { bom.bomFormat = "SPDX"; },
  ];
  for (const change of changes) {
    await fixture("windows-x64", async (input) => {
      change(input.sbom);
      await writeFile(input.sbomPath, JSON.stringify(input.sbom));
      await assert.rejects(() => packageCandidate(input.request));
      await assert.rejects(access(join(input.root, "candidate-assets", input.platform, "manifest.json")), { code: "ENOENT" });
    });
  }
});

test("a previously clean SBOM cannot bless later uncommitted source changes", async () => {
  await fixture("windows-x64", async (input) => {
    await writeFile(join(input.root, "docs", sourceDocs[0]), "uncommitted source change");
    await assert.rejects(() => packageCandidate(input.request), /clean checkout/);
    await assert.rejects(access(join(input.root, "candidate-assets", input.platform, "manifest.json")), { code: "ENOENT" });
  });
});
