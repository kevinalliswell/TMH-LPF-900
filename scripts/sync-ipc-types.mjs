import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const write = process.argv.includes("--write");
const result = spawnSync(
  "cargo",
  [
    "test",
    "-p",
    "tmh-app",
    "--lib",
    "--features",
    "typescript-export",
    "--locked",
    "--",
    "generated_types_match_rust_serde_dtos",
  ],
  {
    cwd: root,
    stdio: "inherit",
    env: {
      ...process.env,
      ...(write ? { UPDATE_TMH_IPC_TYPES: "1" } : {}),
    },
  },
);

if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
