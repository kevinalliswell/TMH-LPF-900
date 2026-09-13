import { useEffect, type FC } from "react";
import {
  getRuntimeSettings,
  subscribeAppNotices,
  subscribeExperimentState,
  subscribeTelemetry,
} from "./ipc";
import { useToasts } from "./stores/toast";
import { useExperiment } from "./stores/experiment";
import { useNav, type View } from "./stores/nav";
import { useTelemetry } from "./stores/telemetry";
import { Toasts } from "./components/Toasts";
import { ExperimentBar } from "./features/control/ExperimentBar";
import { ControlView } from "./features/control/ControlView";
import { HomeView } from "./features/home/HomeView";
import { ModeView } from "./features/mode/ModeView";
import { GroupView } from "./features/groups/GroupView";
import { HistoryView } from "./features/history/HistoryView";
import { SettingsView } from "./features/settings/SettingsView";
import { HelpView } from "./features/help/HelpView";
import { AboutView } from "./features/about/AboutView";
import { StatusBar } from "./features/control/StatusBar";
import { AuthGate } from "./features/auth/AuthGate";
import { UserManagementView } from "./features/users/UserManagementView";
import { lockSession, logout } from "./ipc";
import { useAuth } from "./stores/auth";

const NAV: { view: View; label: string }[] = [
  { view: "home", label: "首页" },
  { view: "control", label: "综合控制" },
  { view: "mode", label: "实验模式" },
  { view: "history", label: "历史查询" },
  { view: "groups", label: "检验组报告" },
  { view: "users", label: "用户管理" },
  { view: "settings", label: "通讯设置" },
  { view: "help", label: "帮助" },
  { view: "about", label: "关于" },
];

const VIEWS: Record<View, FC> = {
  home: HomeView,
  control: ControlView,
  mode: ModeView,
  history: HistoryView,
  groups: GroupView,
  users: UserManagementView,
  settings: SettingsView,
  help: HelpView,
  about: AboutView,
};

export default function App() {
  return (
    <AuthGate>
      <AuthenticatedApp />
    </AuthGate>
  );
}

function AuthenticatedApp() {
  const setSnapshot = useTelemetry((s) => s.setSnapshot);
  const setStaleMs = useTelemetry((s) => s.setStaleMs);
  const setExperiment = useExperiment((s) => s.setState);
  const view = useNav((s) => s.view);
  const setView = useNav((s) => s.setView);
  const leaveBlockReason = useNav((s) => s.leaveBlockReason);
  const auth = useAuth((s) => s.snapshot);
  const refreshAuth = useAuth((s) => s.refresh);
  useEffect(() => subscribeTelemetry(setSnapshot), [setSnapshot]);
  useEffect(() => subscribeExperimentState(setExperiment), [setExperiment]);
  useEffect(
    () =>
      subscribeAppNotices((message) =>
        useToasts.getState().push(message, "warn"),
      ),
    [],
  );
  // Mirror the backend's configured heartbeat timeout rather than hardcoding 5 s (#44).
  useEffect(() => {
    void getRuntimeSettings()
      .then((settings) => setStaleMs(settings.heartbeat_timeout_s * 1000))
      .catch(() => {});
  }, [setStaleMs]);

  const visibleNav = NAV.filter(
    (item) => item.view !== "users" || auth?.user?.role === "ADMIN",
  );
  const effectiveView = visibleNav.some((item) => item.view === view) ? view : "control";
  const title = visibleNav.find((n) => n.view === effectiveView)?.label ?? "";
  const ViewComponent = VIEWS[effectiveView];

  return (
    <div className="flex h-full w-full">
      <nav
        className="w-[168px] shrink-0 flex flex-col border-r"
        style={{ borderColor: "var(--border)", background: "var(--panel)" }}
      >
        <div
          className="px-4 py-4 border-b"
          style={{ borderColor: "var(--border)" }}
        >
          <div className="text-sm font-bold tracking-wide">TMH-LPF-900</div>
          <div className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
            冶金性能检测
          </div>
        </div>
        <ul className="py-2 text-[13px]">
          {visibleNav.map((n) => {
            const active = n.view === effectiveView;
            return (
              <li key={n.view}>
                <button
                  disabled={!!leaveBlockReason && !active}
                  title={!active ? leaveBlockReason ?? undefined : undefined}
                  onClick={() => setView(n.view)}
                  className="w-full text-left px-4 py-2 disabled:opacity-40 disabled:cursor-not-allowed"
                  style={{
                    color: active ? "var(--fg)" : "var(--fg-dim)",
                    background: active ? "var(--panel-2)" : "transparent",
                    borderLeft: active
                      ? "2px solid var(--heat)"
                      : "2px solid transparent",
                  }}
                >
                  {n.label}
                </button>
              </li>
            );
          })}
        </ul>
        {leaveBlockReason && <p className="px-4 text-[11px] text-[var(--warn)]">{leaveBlockReason}</p>}
        <div className="flex-1" />
        <div
          className="px-4 py-3 text-[10px]"
          style={{ color: "var(--fg-faint)" }}
        >
          北京科技大学
        </div>
      </nav>

      <div className="flex flex-col flex-1 min-w-0">
        <header
          className="flex items-center justify-between px-5 h-14 border-b shrink-0"
          style={{ borderColor: "var(--border)" }}
        >
          <h1 className="text-base font-semibold flex items-baseline gap-2">
            {title}
            <span className="mono text-xs" style={{ color: "var(--fg-faint)" }}>
              TMH-LPF-900 v0.2.0
            </span>
          </h1>
          <div className="flex items-center gap-3 text-xs">
            <span style={{ color: "var(--fg-dim)" }}>
              {auth?.user?.display_name} ·{" "}
              {auth?.user?.role === "ADMIN" ? "管理员" : "普通用户"}
            </span>
            <button
              onClick={() => void lockSession().finally(() => refreshAuth())}
              className="px-2 py-1 rounded"
              style={{ border: "1px solid var(--border)" }}
            >
              锁定
            </button>
            <button
              onClick={() => void logout().finally(() => refreshAuth())}
              className="px-2 py-1 rounded"
              style={{ border: "1px solid var(--border)" }}
            >
              退出
            </button>
          </div>
        </header>

        <ExperimentBar />

        <ViewComponent />

        <StatusBar />
      </div>
      <Toasts />
    </div>
  );
}
