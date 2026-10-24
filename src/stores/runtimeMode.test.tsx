import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { getRuntimeMode } from "../ipc";
import { useRuntimeMode } from "./runtimeMode";
vi.mock("../ipc", () => ({ getRuntimeMode: vi.fn() }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
it("exposes an unknown mode after rejection and permits a later mount to retry", async () => {
  vi.mocked(getRuntimeMode).mockRejectedValue(new Error("AUTH_REQUIRED"));
  const first = renderHook(useRuntimeMode);
  await waitFor(() => expect(first.result.current).toBe("unknown"));
  first.unmount();
  vi.mocked(getRuntimeMode).mockResolvedValue("mock");
  const second = renderHook(useRuntimeMode);
  await waitFor(() => expect(second.result.current).toBe("mock"));
});
