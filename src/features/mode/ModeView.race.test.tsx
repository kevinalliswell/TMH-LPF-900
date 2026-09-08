import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ModeView } from "./ModeView";
import { useAuth } from "../../stores/auth";
import { useExperiment } from "../../stores/experiment";
import { mockAuthState, mockBootstrapAdmin } from "../../ipc/authMock";
import { mockListPrograms } from "../../ipc/mock";
import * as ipc from "../../ipc";
import type { GasLimits, ProgramDto } from "../../ipc/types";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("keeps a copy made at first display after slow loading and preserves edits when limits arrive", async () => {
  mockBootstrapAdmin("race-admin", "竞态核验员", "test-password");
  useAuth.setState({ snapshot: mockAuthState(), loading: false });
  useExperiment.setState({ state: { running: false } });
  let loadPrograms!: (items: ProgramDto[]) => void;
  let loadLimits!: (limits: GasLimits) => void;
  vi.spyOn(ipc, "listPrograms").mockReturnValue(new Promise((resolve) => {
    loadPrograms = resolve;
  }));
  vi.spyOn(ipc, "getGasSafetyLimits").mockReturnValue(new Promise((resolve) => {
    loadLimits = resolve;
  }));

  let copied = false;
  let connected = false;
  const observer = new MutationObserver(() => {
    const button = screen.queryByRole("button", {
      name: "复制为自定义",
    }) as HTMLButtonElement | null;
    if (button && !button.disabled && !copied) {
      copied = true;
      connected = button.isConnected;
      observer.disconnect();
      // Click the committed button before passive effects run. Wrapping this in
      // act/fireEvent would flush those effects first and hide the reported race.
      button.click();
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
  try {
    render(<ModeView />);
    expect(screen.queryByRole("button", { name: "复制为自定义" })).toBeNull();
    loadPrograms(mockListPrograms());
    const reason = await screen.findByRole("textbox", { name: "本次修改原因" });
    expect(copied && connected).toBe(true);
    fireEvent.change(reason, { target: { value: "保留首次复制后的研究条件" } });
    fireEvent.change(screen.getByLabelText("第 3 阶段温度 (℃)"), {
      target: { value: "850" },
    });
    await act(async () => {
      loadLimits({ h2: 5, co: 5, flow_steps: [0.01, 0.1, 0.01, 0.01] });
    });
    expect((screen.getByLabelText("本次修改原因") as HTMLTextAreaElement).value)
      .toBe("保留首次复制后的研究条件");
    expect((screen.getByLabelText("第 3 阶段温度 (℃)") as HTMLInputElement).value)
      .toBe("850");
    expect((screen.getByRole("button", { name: "保存修改" }) as HTMLButtonElement).disabled)
      .toBe(false);
  } finally {
    observer.disconnect();
  }
});
