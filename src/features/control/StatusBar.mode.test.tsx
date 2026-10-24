import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useRuntimeMode } from "../../stores/runtimeMode";
import { StatusBar } from "./StatusBar";
vi.mock("../../stores/runtimeMode", () => ({ useRuntimeMode: vi.fn() }));
afterEach(cleanup);
it("makes a failed or unresolved mode visible without labelling it mock", () => {
  for (const mode of [null, "unknown"] as const) {
    vi.mocked(useRuntimeMode).mockReturnValue(mode);
    const view = render(<StatusBar />);
    expect(screen.getByText("运行模式未确认")).toBeTruthy();
    expect(screen.queryByText("模拟模式")).toBeNull();
    view.unmount();
  }
});
it("renders the confirmed effective mode", () => {
  vi.mocked(useRuntimeMode).mockReturnValue("mock");
  render(<StatusBar />);
  expect(screen.getByText("模拟模式")).toBeTruthy();
  expect(screen.queryByText("运行模式未确认")).toBeNull();
});
