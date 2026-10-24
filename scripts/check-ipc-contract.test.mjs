import { test } from "node:test";
import assert from "node:assert/strict";
import { checkIpcContract } from "./check-ipc-contract.mjs";
const rust = (attribute = '(rename_all = "snake_case")', result = "CommandResult<()>") => `
#[tauri::command${attribute}]
fn query(state: State<AppState>, max_points: usize, filter: Option<String>) -> ${result} { Ok(()) }
fn app() { tauri::generate_handler![query] }
`;
test("injected state is excluded and optional payload keys may be omitted", () => {
  assert.deepEqual(checkIpcContract(rust(), 'invoke("query", { max_points: 4 })').errors, []);
});
test("snake_case and default camelCase are distinct contracts", () => {
  assert.equal(checkIpcContract(rust(), 'invoke("query", { maxPoints: 4 })').errors.length, 2);
  assert.deepEqual(checkIpcContract(rust(""), 'invoke("query", { maxPoints: 4 })').errors, []);
});
test("missing required and unknown injected keys are rejected", () => {
  const result = checkIpcContract(rust(), 'invoke("query", { state: {} })');
  assert(result.errors.some((line) => line.includes('expects argument key "max_points"')));
  assert(result.errors.some((line) => line.includes('unknown argument key "state"')));
});
test("unregistered commands and raw-string error returns cannot pass", () => {
  assert(checkIpcContract(rust().replace("generate_handler![query]", "generate_handler![]"), 'invoke("query", { max_points: 4 })').errors.some((line) => line.includes("not registered")));
  assert.throws(() => checkIpcContract(rust(undefined, "Result<(), String>"), 'invoke("query", { max_points: 4 })'), /must return CommandResult/);
});
test("unknown invocations fail and macro mentions in comments do not invent commands", () => {
  assert(checkIpcContract(rust(), 'invoke("unknown")').errors.some((line) => line.includes("no such")));
  const source = '// #[tauri::command]\n' + rust();
  assert.equal(checkIpcContract(source, 'invoke("query", { max_points: 4 })').commands, 1);
});
