import { Page, SectionTitle } from "../../components/Page";

const STEPS = [
  ["1 · 装样与去皮", "在综合控制页确认天平读数稳定后执行去皮（A00），装入试样并记录初始重量。"],
  ["2 · 选择实验模式", "点击「开始实验」，选择国标模式（13241 还原性 / 13242 低温粉化 / 13240 自由膨胀），填写样品信息。"],
  ["3 · 自动运行", "系统按程序自动升温→恒温→还原→冷却，逐阶段切换气氛并采样入库。可「跳过阶段」或「停止实验」。"],
  ["4 · 结果分析", "在历史查询页选择实验，录入所需参数（O₂% / 筛分质量 / 体积对），计算并保存国标指标。"],
];

const SAFETY = [
  "H₂ / CO 设定值在 MFC 总线建帧时钳制，前端仅展示限值，不参与安全判定。",
  "实验的任何退出路径（完成 / 停止 / 异常 / 崩溃恢复）都会切换保护气氛：N₂ 5.0，H₂=CO₂=CO=0。",
  "零流量指令始终放行，紧急切断永不被钳制阻挡。",
];

export function HelpView() {
  return (
    <Page>
      <div className="panel p-4">
        <SectionTitle>操作流程</SectionTitle>
        <div className="flex flex-col gap-3">
          {STEPS.map(([t, d]) => (
            <div key={t}>
              <div className="text-[13px] font-medium" style={{ color: "var(--heat)" }}>
                {t}
              </div>
              <div className="text-[13px] mt-0.5" style={{ color: "var(--fg-dim)" }}>
                {d}
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel p-4">
        <SectionTitle>安全须知</SectionTitle>
        <ul className="flex flex-col gap-2">
          {SAFETY.map((s) => (
            <li key={s} className="flex gap-2 text-[13px]" style={{ color: "var(--fg-dim)" }}>
              <span style={{ color: "var(--danger)" }}>▸</span>
              <span>{s}</span>
            </li>
          ))}
        </ul>
      </div>

      <div className="panel p-4">
        <SectionTitle>联系</SectionTitle>
        <div className="text-[13px]" style={{ color: "var(--fg-dim)" }}>
          北京科技大学 · 冶金性能检测系统 TMH-LPF-900
        </div>
      </div>
    </Page>
  );
}
