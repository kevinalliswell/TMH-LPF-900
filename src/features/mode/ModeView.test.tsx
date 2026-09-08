import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ModeView } from "./ModeView";
import { useAuth } from "../../stores/auth";
import { useExperiment } from "../../stores/experiment";
import { mockAuthState, mockBootstrapAdmin } from "../../ipc/authMock";
import {
  mockListPrograms,
  mockListCustomProgramRevisions,
} from "../../ipc/mock";
afterEach(cleanup);
it("saves two explained revisions through the form while protecting the standard ending", async () => {
  mockBootstrapAdmin("mode-admin", "界面核验员", "test-password");
  useAuth.setState({ snapshot: mockAuthState(), loading: false });
  useExperiment.setState({ state: { running: false } });
  render(<ModeView />);
  fireEvent.click(await screen.findByRole("button", { name: "复制为自定义" }));
  const reason = await screen.findByRole("textbox", { name: "本次修改原因" });
  const save = screen.getByRole("button", { name: "保存修改" });
  expect((save as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "添加阶段" })).toBeNull();
  expect(screen.queryByLabelText("第 4 阶段N₂流量")).toBeNull();
  expect(screen.queryByLabelText("第 1 阶段时间 (min)")).toBeNull();
  fireEvent.change(screen.getByLabelText("第 3 阶段温度 (℃)"), {
    target: { value: "850" },
  });
  fireEvent.change(reason, { target: { value: "对比850℃" } });
  await waitFor(() => expect((save as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(save);
  await waitFor(() =>
    expect(
      mockListPrograms().find((p) => p.source === "custom")?.provenance
        ?.revision,
    ).toBe(1),
  );
  await waitFor(() =>
    expect(
      (screen.getByLabelText("本次修改原因") as HTMLTextAreaElement).value,
    ).toBe(""),
  );
  fireEvent.change(screen.getByLabelText("第 3 阶段温度 (℃)"), {
    target: { value: "800" },
  });
  fireEvent.change(screen.getByLabelText("本次修改原因"), {
    target: { value: "继续对比800℃" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存修改" }));
  await waitFor(() =>
    expect(
      mockListPrograms().find((p) => p.source === "custom")?.provenance
        ?.revision,
    ).toBe(2),
  );
  const saved = mockListPrograms().find((p) => p.source === "custom")!;
  expect(
    mockListCustomProgramRevisions(saved.id).map(
      (p) => p.stages[2]!.target_temp,
    ),
  ).toEqual([800, 850]);
  fireEvent.click(screen.getByRole("button", { name: "版本历史" }));
  expect(await screen.findByText("版本 1 · 界面核验员")).toBeTruthy();
});
