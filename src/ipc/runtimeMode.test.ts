import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { getRuntimeMode } from "./index";

vi.mock("@tauri-apps/api/core", async (original) => ({
  ...await original<typeof import("@tauri-apps/api/core")>(), invoke: vi.fn(),
}));
afterEach(() => { vi.unstubAllGlobals(); vi.resetAllMocks(); });
it("never substitutes mock after a failed native mode query", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.mocked(invoke).mockRejectedValue("AUTH_REQUIRED");
  await expect(getRuntimeMode()).rejects.toThrow("AUTH_REQUIRED");
});
it("displays only the effective transport mode from the runtime DTO", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.mocked(invoke).mockResolvedValue({ requested_mode: "mock", effective_mode: "mock" });
  await expect(getRuntimeMode()).resolves.toBe("mock");
});
it("rejects missing or unrecognized effective mode rather than inventing a label", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  for (const value of ["mock", {}, { requested_mode: "real", effective_mode: "garbage" }]) {
    vi.mocked(invoke).mockResolvedValue(value);
    await expect(getRuntimeMode()).rejects.toThrow();
  }
});
