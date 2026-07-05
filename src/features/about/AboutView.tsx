import { Page, SectionTitle } from "../../components/Page";

const STANDARDS = [
  ["GB/T 13241", "铁矿石 还原性的测定", "还原度 Rt、还原速率指数"],
  ["GB/T 13242", "铁矿石 低温还原粉化的测定", "RDI +6.3 / +3.15 / −0.5"],
  ["GB/T 13240", "铁矿石 自由膨胀指数的测定", "自由膨胀指数 SI"],
];

export function AboutView() {
  return (
    <Page>
      <div className="panel p-6">
        <div className="flex items-baseline gap-3">
          <div className="text-2xl font-bold tracking-tight">TMH-LPF-900</div>
          <div className="mono text-[12px]" style={{ color: "var(--fg-faint)" }}>
            v0.1.0
          </div>
        </div>
        <div className="text-[13px] mt-1" style={{ color: "var(--fg-dim)" }}>
          铁矿石冶金性能检测与控制系统
        </div>
        <div className="mono text-[12px] mt-3" style={{ color: "var(--fg-faint)" }}>
          Tauri v2 · React + TypeScript · Rust（tmh-core / tmh-protocols）
        </div>
      </div>

      <div className="panel p-5">
        <SectionTitle>执行标准</SectionTitle>
        <div className="flex flex-col">
          {STANDARDS.map(([code, title, out]) => (
            <div
              key={code}
              className="flex items-baseline gap-3 py-2"
              style={{ borderTop: "1px solid var(--border)" }}
            >
              <span className="mono text-[13px] w-24 shrink-0" style={{ color: "var(--heat)" }}>
                {code}
              </span>
              <div>
                <div className="text-[13px]">{title}</div>
                <div className="text-[12px]" style={{ color: "var(--fg-faint)" }}>
                  {out}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel p-5">
        <SectionTitle>研制</SectionTitle>
        <div className="text-[13px]" style={{ color: "var(--fg-dim)" }}>
          北京科技大学（USTB）
        </div>
        <div className="text-[12px] mt-1" style={{ color: "var(--fg-faint)" }}>
          9 通道温度控制 · 4 路 Azbil MFC 气体流量 · RS232 电子天平
        </div>
      </div>
    </Page>
  );
}
