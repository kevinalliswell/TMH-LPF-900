import { useEffect } from "react";
import { subscribeTelemetry } from "./ipc";
import { useTelemetry } from "./stores/telemetry";
import { ControlPanel } from "./features/control/ControlPanel";
import { MonitorPanel } from "./features/control/MonitorPanel";
import { StatusBar } from "./features/control/StatusBar";
import { TempChart } from "./features/control/TempChart";

const NAV = ["首页", "综合控制", "实验模式", "历史查询", "通讯设置", "帮助", "关于"];

export default function App() {
  const setSnapshot = useTelemetry((s) => s.setSnapshot);
  useEffect(() => subscribeTelemetry(setSnapshot), [setSnapshot]);

  return (
    <div className="flex h-full w-full">
      <nav
        className="w-[168px] shrink-0 flex flex-col border-r"
        style={{ borderColor: "var(--border)", background: "var(--panel)" }}
      >
        <div className="px-4 py-4 border-b" style={{ borderColor: "var(--border)" }}>
          <div className="text-sm font-bold tracking-wide">TMH-LPF-900</div>
          <div className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
            冶金性能检测
          </div>
        </div>
        <ul className="py-2 text-[13px]">
          {NAV.map((n, i) => (
            <li
              key={n}
              className="px-4 py-2 cursor-default"
              style={{
                color: i === 1 ? "var(--fg)" : "var(--fg-dim)",
                background: i === 1 ? "var(--panel-2)" : "transparent",
                borderLeft: i === 1 ? "2px solid var(--heat)" : "2px solid transparent",
              }}
            >
              {n}
            </li>
          ))}
        </ul>
        <div className="flex-1" />
        <div className="px-4 py-3 text-[10px]" style={{ color: "var(--fg-faint)" }}>
          北京科技大学
        </div>
      </nav>

      <div className="flex flex-col flex-1 min-w-0">
        <header
          className="flex items-center justify-between px-5 h-14 border-b"
          style={{ borderColor: "var(--border)" }}
        >
          <h1 className="text-base font-semibold flex items-baseline gap-2">
            综合控制
            <span className="mono text-xs" style={{ color: "var(--fg-faint)" }}>
              TMH-LPF-900 v0.1.0
            </span>
          </h1>
        </header>

        <main
          className="flex-1 min-h-0 grid gap-3 p-3"
          style={{ gridTemplateColumns: "1fr 320px", gridTemplateRows: "1fr 250px" }}
        >
          <div className="min-h-0 overflow-auto">
            <MonitorPanel />
          </div>
          <div className="row-span-2 min-h-0 overflow-auto">
            <ControlPanel />
          </div>
          <div className="panel min-h-0">
            <TempChart />
          </div>
        </main>

        <StatusBar />
      </div>
    </div>
  );
}
