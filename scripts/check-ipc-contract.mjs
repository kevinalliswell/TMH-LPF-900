// Guards the Webview <-> Rust IPC boundary (#40).
//
// Tauri derives each command's argument key from the Rust parameter name and converts it
// to lowerCamelCase, unless the command opts out with `#[tauri::command(rename_all =
// "snake_case")]`. A key the frontend spells differently is not a type error on either
// side: the command simply fails at runtime with `missing required key ...`, and neither
// the Rust tests nor the frontend tests (which run against the browser mock) can see it.
//
// This script recomputes the expected key for every command and compares it with the keys
// `src/ipc/index.ts` actually sends.

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUST = "src-tauri/src/lib.rs";
const TS = "src/ipc/index.ts";

/** Parameter types injected by Tauri rather than sent from JS. */
const INJECTED = [/^AppHandle\b/, /^State\s*</, /^Window\b/, /^WebviewWindow\b/];

/** heck's `to_lower_camel_case`, as applied by the tauri command macro. */
function lowerCamel(name) {
  const [head, ...rest] = name.split("_").filter((part) => part !== "");
  if (head === undefined) return "";
  return head + rest.map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join("");
}

/** True when `index` sits inside a `//` line comment (doc comments included). */
function isCommented(source, index) {
  const lineStart = source.lastIndexOf("\n", index) + 1;
  return source.slice(lineStart, index).includes("//");
}

/** Read a balanced (...) group starting at `open`; returns [inner, indexAfterClose]. */
function readGroup(text, open, [begin, end] = ["(", ")"]) {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    if (text[i] === begin) depth += 1;
    else if (text[i] === end) {
      depth -= 1;
      if (depth === 0) return [text.slice(open + 1, i), i + 1];
    }
  }
  throw new Error(`unbalanced ${begin} at offset ${open}`);
}

/** Split on commas that are not nested inside (), <>, [] or {}. */
function splitTopLevel(text) {
  const parts = [];
  let depth = 0;
  let current = "";
  for (const ch of text) {
    if ("(<[{".includes(ch)) depth += 1;
    else if (")>]}".includes(ch)) depth -= 1;
    if (ch === "," && depth === 0) {
      parts.push(current);
      current = "";
    } else current += ch;
  }
  if (current.trim() !== "") parts.push(current);
  return parts.map((part) => part.trim()).filter((part) => part !== "");
}

function parseRustCommands(source) {
  const commands = new Map();
  const attribute = /#\[tauri::command(\([^\]]*\))?\]/g;
  let match;
  while ((match = attribute.exec(source)) !== null) {
    if (isCommented(source, match.index)) continue; // e.g. a doc comment mentioning the macro
    const snakeCase = (match[1] ?? "").includes('rename_all = "snake_case"');
    const signature = /(?:pub\s+)?(?:async\s+)?fn\s+(\w+)(?:<[^>]+>)?\s*\(/g;
    signature.lastIndex = attribute.lastIndex;
    const found = signature.exec(source);
    if (!found || found.index > attribute.lastIndex + 40) {
      throw new Error(`no fn found after ${match[0]}`);
    }
    const [inner, afterParams] = readGroup(source, signature.lastIndex - 1);
    const returnType = source.slice(afterParams, source.indexOf("{", afterParams)).trim();
    if (returnType.includes("Result<") && !returnType.startsWith("-> CommandResult<")) {
      throw new Error(`${found[1]}: fallible commands must return CommandResult<T>`);
    }
    const params = splitTopLevel(inner)
      .map((param) => {
        const colon = param.indexOf(":");
        const name = param.slice(0, colon).trim();
        const type = param.slice(colon + 1).trim();
        return { name, type };
      })
      .filter(({ type }) => !INJECTED.some((pattern) => pattern.test(type)))
      .map(({ name, type }) => ({
        key: snakeCase ? name : lowerCamel(name),
        optional: type.startsWith("Option<"),
      }));
    commands.set(found[1], { params, snakeCase });
  }
  return commands;
}

function parseRegisteredHandlers(source) {
  const at = source.indexOf("tauri::generate_handler![");
  if (at < 0) throw new Error("generate_handler! not found");
  const [inner] = readGroup(source, source.indexOf("[", at), ["[", "]"]);
  return new Set(splitTopLevel(inner));
}

/** Every `invoke("name", { ...keys })` in the TS IPC module. */
function parseInvocations(source) {
  const calls = [];
  const invoke = /\binvoke(?:<[^>]*>)?\s*\(/g;
  let match;
  while ((match = invoke.exec(source)) !== null) {
    if (isCommented(source, match.index)) continue;
    const [inner] = readGroup(source, invoke.lastIndex - 1);
    const args = splitTopLevel(inner);
    const name = args[0]?.match(/^["'`](\w+)["'`]$/)?.[1];
    if (!name) continue;
    const payload = args[1];
    let keys = [];
    if (payload?.startsWith("{")) {
      const [object] = readGroup(payload, 0, ["{", "}"]);
      keys = splitTopLevel(object).map((entry) => {
        const colon = entry.indexOf(":");
        return (colon < 0 ? entry : entry.slice(0, colon)).trim();
      });
    }
    calls.push({ name, keys, hasPayload: payload !== undefined });
  }
  return calls;
}

export function checkIpcContract(rust, ts) {
const commands = parseRustCommands(rust);
const registered = parseRegisteredHandlers(rust);
const calls = parseInvocations(ts);

const errors = [];
const notes = [];

for (const name of registered) {
  if (!commands.has(name)) errors.push(`${name}: registered in generate_handler! but has no #[tauri::command] fn`);
}

const invoked = new Set();
for (const { name, keys, hasPayload } of calls) {
  invoked.add(name);
  const command = commands.get(name);
  if (!command) {
    errors.push(`${name}: invoked from ${TS} but no such #[tauri::command]`);
    continue;
  }
  if (!registered.has(name)) {
    errors.push(`${name}: invoked from ${TS} but not registered in generate_handler!`);
  }
  const expected = new Set(command.params.map((p) => p.key));
  for (const { key, optional } of command.params) {
    if (!keys.includes(key) && !optional) {
      const sent = keys.length ? keys.join(", ") : hasPayload ? "(empty object)" : "(no payload)";
      errors.push(
        `${name}: Rust expects argument key "${key}", frontend sends ${sent}` +
          (command.snakeCase ? "" : ` — the command has no rename_all, so keys are camelCased`),
      );
    }
  }
  for (const key of keys) {
    if (!expected.has(key)) errors.push(`${name}: frontend sends unknown argument key "${key}"`);
  }
}

for (const name of registered) {
  if (!invoked.has(name)) {
    notes.push(`command ${name} is registered but never invoked from ${TS}`);
  }
}

return { errors, notes, invocations: calls.length, commands: commands.size };
}

async function main() {
  const [rust, ts] = await Promise.all([readFile(RUST, "utf8"), readFile(TS, "utf8")]);
  const result = checkIpcContract(rust, ts);
  for (const note of result.notes) console.warn(`note: ${note}`);
  if (result.errors.length) throw new Error(result.errors.join("\n"));
  console.log(`IPC contract OK: ${result.invocations} invocations across ${result.commands} commands`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(`IPC contract error: ${error.message}`); process.exitCode = 1; });
}
