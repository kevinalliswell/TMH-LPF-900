import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { execFileSync } from "node:child_process";

test("verification context retains parents of a shallow checkout without collecting arbitrary environment", async () => {
  const root = await mkdtemp(join(tmpdir(), "tmh-context-"));
  const repo = join(root, "repo"), shallow = join(root, "shallow");
  const git = (cwd, ...args) => execFileSync("git", ["-c", "user.name=Context Test", "-c", "user.email=context@example.invalid", ...args], { cwd, encoding: "utf8", stdio: "pipe" }).trim();
  try {
    git(root, "init", "-b", "main", repo);
    await writeFile(join(repo, "package.json"), JSON.stringify({ version: "0.2.0", packageManager: "pnpm@11.10.0" }));
    git(repo, "add", "."); git(repo, "commit", "-m", "first");
    const parent = git(repo, "rev-parse", "HEAD");
    await writeFile(join(repo, "file"), "second commit");
    git(repo, "add", "."); git(repo, "commit", "-m", "second");
    git(root, "clone", "--depth", "1", pathToFileURL(repo).href, shallow);
    assert.equal(git(shallow, "rev-parse", "--is-shallow-repository"), "true");
    const output = join(root, "context.json");
    execFileSync(process.execPath, [fileURLToPath(new URL("./verification-context.mjs", import.meta.url)), output], {
      cwd: shallow, env: { ...process.env, TMH_SECRET_SENTINEL: "never-collect-this" }, stdio: "pipe",
    });
    const text = await readFile(output, "utf8"), value = JSON.parse(text);
    assert.deepEqual(value.source_parents, [parent]);
    assert.equal(value.source_commit, git(repo, "rev-parse", "HEAD"));
    assert.equal(value.working_tree_dirty, false);
    assert(!text.includes("never-collect-this"));
  } finally { await rm(root, { recursive: true, force: true }); }
});
