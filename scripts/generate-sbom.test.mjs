import { test } from "node:test";
import assert from "node:assert/strict";
import { validateLockCoverage } from "./generate-sbom.mjs";

const cargo = 'version = 4\n[[package]]\nname = "tmh-core"\nversion = "0.2.0"\n[[package]]\nname = "windows-sys"\nversion = "0.61.2"\n';
const pkg = { dependencies: { react: "^19" } };
const bom = () => ({
  bomFormat: "CycloneDX", specVersion: "1.7",
  metadata: { tools: { components: [{ name: "syft", version: "1.55.0" }] } },
  components: [{ name: "tmh-core", version: "0.2.0" }, { name: "windows-sys", version: "0.61.2" }, { name: "react", version: "19.2.7", purl: "pkg:npm/react@19.2.7" }],
});
test("SBOM coverage includes foreign platform crates and resolved production frontend dependencies", () => {
  assert.doesNotThrow(() => validateLockCoverage(bom(), cargo, pkg));
  for (const name of ["windows-sys", "react"]) {
    const input = bom(); input.components = input.components.filter((component) => component.name !== name);
    assert.throws(() => validateLockCoverage(input, cargo, pkg), /Incomplete lockfile/);
  }
  const input = bom(); input.metadata.tools.components[0].version = "1.0.0";
  assert.throws(() => validateLockCoverage(input, cargo, pkg), /pinned Syft/);
  assert.throws(() => validateLockCoverage(bom(), "", pkg), /Cargo.lock/);
});
