import { Page, SectionTitle } from "../../components/Page";

const STEPS = [
  [
    "1 · 确认数据来源",
    "当前版本用于桌面模拟验收，尚未接通真实设备。首次启动创建管理员，没有默认口令；正式实验须先完成设备与方法验收。",
  ],
  [
    "2 · 选择程序与记录试样",
    "选择三项国标模板并填写样品资料。自定义程序从国标派生，须记录修改原因；首版可调整前三段温度和气氛条件，结束步骤保留。",
  ],
  [
    "3 · 按阶段操作",
    "温度程序由仪表执行，上位机记录读值并管理气氛流程。恒温需要连续稳定，人工步骤须按界面确认；保护尾段不能跳过。模拟运行不代表仪表已收到温控参数。",
  ],
  [
    "4 · 单次分析与补录",
    "在历史查询中按方法补录称量及 FeO / 全铁、转鼓筛分或整份体积数据，并填写数据来源、制样、校准和外部操作记录。保存分析后可导出单次报告与原始数据。",
  ],
  [
    "5 · 检验组与最终结果",
    "在桌面应用的「检验组报告」选择同批独立试验，填写材料及所需重复性限值依据。预览待补测和采用/排除结果，补录实验室、报告编号与复核资料，保存版本后导出 HTML / JSON 或打印为 PDF。",
  ],
];

const SAFETY = [
  "停止、故障与紧急停机申请保护气氛并锁存；正常完成保留本方法规定的结束气氛。保护失败不能当作已经安全结束。",
  "解除保护锁存前须无运行及待保存实验，且温度、四路流量新鲜有效，非氮气流量与确认设定满足复位条件。",
  "数据保存失败时先重试保存；未保存状态会阻止新实验、删除、更新和退出。",
  "数值重复性通过不等于整个方法或采购性能合格。缺测、自定义、模拟数据及未填写的外部证据须保留在报告中，由实验室复核。",
];

export function HelpView() {
  return (
    <Page>
      <div className="panel p-4">
        <SectionTitle>操作流程</SectionTitle>
        <div className="flex flex-col gap-3">
          {STEPS.map(([t, d]) => (
            <div key={t}>
              <div
                className="text-[13px] font-medium"
                style={{ color: "var(--heat)" }}
              >
                {t}
              </div>
              <div
                className="text-[13px] mt-0.5"
                style={{ color: "var(--fg-dim)" }}
              >
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
            <li
              key={s}
              className="flex gap-2 text-[13px]"
              style={{ color: "var(--fg-dim)" }}
            >
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
