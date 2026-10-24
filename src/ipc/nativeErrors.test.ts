import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { getCommConfig, login } from "./index";
import { CommandFailure } from "./errors";

vi.mock("@tauri-apps/api/core", async (original) => ({
  ...await original<typeof import("@tauri-apps/api/core")>(),
  invoke: vi.fn(),
}));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

it("normalizes native login rejections through the public IPC function", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.mocked(invoke).mockRejectedValue({ code: "AUTH_FAILED", message_zh: "用户名或密码错误", detail: null });
  const error = await login("alice", "password").catch((cause: unknown) => cause);
  expect(error).toBeInstanceOf(CommandFailure);
  expect(error).toMatchObject({ code: "AUTH_FAILED", message: "用户名或密码错误", detail: null });
});

it("keeps pre-dispatch Tauri errors readable through the same wrapper", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.mocked(invoke).mockRejectedValue("invalid args for command get_comm_config");
  await expect(getCommConfig()).rejects.toThrow("invalid args for command get_comm_config");
});
