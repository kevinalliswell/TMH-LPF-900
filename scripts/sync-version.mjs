#!/usr/bin/env node
// Single-source version sync: package.json.version -> runtime/package display points.
//   node scripts/sync-version.mjs           write mode (updates files)
//   node scripts/sync-version.mjs --check    verify mode (exit 1 on drift)
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const version = pkg.version;
const title = `TMH-LPF-900 v${version}`;

const targets = [
  {
    file: "Cargo.toml",
    transform: (s) =>
      s.replace(
        /(\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+("\s*$)/m,
        `$1${version}$2`,
      ),
  },
  {
    file: "src-tauri/tauri.conf.json",
    transform: (s) => {
      const c = JSON.parse(s);
      c.version = version;
      c.app.windows[0].title = title;
      return JSON.stringify(c, null, 2) + "\n";
    },
  },
  {
    file: "index.html",
    transform: (s) => s.replace(/<title>[^<]*<\/title>/, `<title>${title}</title>`),
  },
  {
    file: "src/App.tsx",
    transform: (s) => s.replace(/TMH-LPF-900 v\d+\.\d+\.\d+/g, title),
  },
  {
    // Browser-preview fallback version (no backend to report it) — keep it in sync too (#13).
    file: "src/ipc/index.ts",
    transform: (s) =>
      s.replace(/const APP_VERSION = "\d+\.\d+\.\d+"/, `const APP_VERSION = "${version}"`),
  },
];

let drift = false;
for (const { file, transform } of targets) {
  const path = join(root, file);
  const cur = readFileSync(path, "utf8");
  const next = transform(cur);
  if (cur === next) {
    console.log(`ok       ${file}`);
  } else if (check) {
    console.error(`MISMATCH ${file} (expected ${version})`);
    drift = true;
  } else {
    writeFileSync(path, next);
    console.log(`updated  ${file}`);
  }
}

if (check && drift) {
  console.error("\nVersion drift. Run: pnpm sync-version");
  process.exit(1);
}
console.log(`\nAll display points at version ${version}.`);
