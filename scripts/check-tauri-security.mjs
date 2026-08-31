import { readFile } from "node:fs/promises";

const [tauriConfig, capability] = await Promise.all([
  readJson("src-tauri/tauri.conf.json"),
  readJson("src-tauri/capabilities/default.json"),
]);

const errors = [];
const csp = tauriConfig?.app?.security?.csp;
if (typeof csp !== "string" || csp.trim() === "") {
  errors.push("src-tauri/tauri.conf.json must define a non-empty CSP");
}

const forbiddenWebviewPermissions = new Set([
  "process:default",
  "updater:default",
]);
for (const permission of capability.permissions ?? []) {
  if (forbiddenWebviewPermissions.has(permission)) {
    errors.push(`webview capability must not grant ${permission}`);
  }
}

if (errors.length > 0) {
  for (const error of errors) {
    console.error(`security configuration error: ${error}`);
  }
  process.exitCode = 1;
} else {
  console.log("Tauri security configuration is constrained");
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}
