import type { ReactNode } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "./App";
import { useAuth } from "./stores/auth";
import { useNav } from "./stores/nav";
import * as ipc from "./ipc";

vi.mock("./features/auth/AuthGate", () => ({ AuthGate: ({children}: {children: ReactNode}) => children }));
vi.mock("./features/control/ControlView", () => ({ ControlView: () => <div>普通控制页面</div> }));
vi.mock("./features/users/UserManagementView", () => ({ UserManagementView: () => <div>管理员用户表单</div> }));
vi.mock("./ipc", async (original) => ({ ...await original<typeof ipc>(),
  subscribeTelemetry: () => () => {}, subscribeExperimentState: () => () => {}, subscribeAppNotices: () => () => {},
  getRuntimeSettings: async () => ({ sampling_interval_s: 1, telemetry_interval_s: 0.2, heartbeat_timeout_s: 5, idle_timeout_s: 900 }),
}));
afterEach(cleanup);

it("does not restore an admin-only screen when the next authenticated user has a regular role", async () => {
  const user = {user_id: "admin", username: "admin", display_name: "管理员", role: "ADMIN" as const, is_active: true, must_change_password: false, created_at: "2026-09-13"};
  useNav.setState({view: "users", leaveBlockReason: null});
  useAuth.setState({snapshot: {status: "AUTHENTICATED", idle_timeout_s: 900, user}});
  await act(async () => { render(<App />); });
  expect(screen.getByText("管理员用户表单")).toBeTruthy();
  act(() => useAuth.setState({snapshot: {status: "AUTHENTICATED", idle_timeout_s: 900, user: {...user, user_id: "operator", role: "USER"}}}));
  expect(screen.queryByText("管理员用户表单")).toBeNull();
  expect(screen.getByText("普通控制页面")).toBeTruthy();
});
