import { useEffect, useState } from "react";
import { getGasSafetyLimits, runtimeMode } from "../../ipc";
import type { GasLimits } from "../../ipc/types";
import { Page, SectionTitle } from "../../components/Page";

const BUSES = [
  {
    name: "温度控制器",
    detail: "Modbus-RTU · 9600 8N1 · 从站地址 1 · 9 通道（T1–T9，T8 为样品温度）",
  },
  {
    name: "气体流量 MFC",
    detail: "Azbil CPL ASCII · 4 路（H₂ / N₂ / CO₂ / CO）· 二补校验",
  },
  { name: "电子天平", detail: "RS232 · 连续输出（G S 稳定标志）· 去皮 A00" },
];

export function SettingsView() {
  const [limits, setLimits] = useState<GasLimits | null>(null);
  useEffect(() => {
    getGasSafetyLimits()
      .then(setLimits)
      .catch(() => {});
  }, []);

  return (
    <Page>
      <div className="text-[13px]" style={{ color: "var(--fg-dim)" }}>
        通讯与安全参数（当前为只读展示；串口连接由后端按 config/comm_config.json 装载）。
      </div>

      <div className="panel p-4">
        <SectionTitle>串口总线</SectionTitle>
        <div className="flex flex-col gap-2">
          {BUSES.map((b) => (
            <div
              key={b.name}
              className="flex items-baseline gap-3 py-1.5"
              style={{ borderTop: "1px solid var(--border)" }}
            >
              <span className="text-[13px] w-28 shrink-0">{b.name}</span>
              <span className="mono text-[12px]" style={{ color: "var(--fg-dim)" }}>
                {b.detail}
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="panel p-4">
        <SectionTitle>安全气氛限值</SectionTitle>
        <div className="flex gap-8">
          <div className="flex flex-col">
            <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
              H₂ 上限
            </span>
            <span className="mono text-[18px]" style={{ color: "var(--danger)" }}>
              {limits ? `${limits.h2} L/min` : "…"}
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
              CO 上限
            </span>
            <span className="mono text-[18px]" style={{ color: "var(--danger)" }}>
              {limits ? `${limits.co} L/min` : "…"}
            </span>
          </div>
        </div>
        <div className="text-[12px] mt-3" style={{ color: "var(--fg-faint)" }}>
          限值在 MFC 总线建帧时强制执行（唯一钳制点），并有编译内置绝对上限兜底。零流量始终放行。
        </div>
      </div>

      <div className="panel p-4">
        <SectionTitle>运行</SectionTitle>
        <div className="mono text-[12px]" style={{ color: "var(--fg-dim)" }}>
          设备模式: {runtimeMode() === "web" ? "web（浏览器模拟）" : "mock（后端模拟设备）"}
        </div>
      </div>
    </Page>
  );
}
