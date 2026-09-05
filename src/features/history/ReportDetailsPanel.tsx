import { useState } from "react";
import { saveReportDetails } from "../../ipc";
import type { ExperimentDetail, ReportDetails } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";

const fields: [keyof ReportDetails, string][] = [
  ["lab_name", "实验室名称"],
  ["lab_address", "实验室地址"],
  ["report_number", "报告编号"],
  ["report_date", "报告日期"],
  ["sample_preparation", "制样、粒度、干燥制度、初始称量与颗粒数"],
  ["equipment", "设备编号、温度测点对应、检定记录"],
  ["gas_and_calibration", "气体纯度、流量标态、MFC 标定/校准记录"],
  ["external_steps", "提炉、冷却、转鼓、筛分、体积测定等实际操作"],
  ["deviations", "实验条件偏离、原因及处理（无偏离请填“无”）"],
  ["reviewer", "复核与签署人（仅录入姓名）"],
  ["repeat_group_reference", "独立重复试验编号及验收记录"],
];
export function ReportDetailsPanel({
  detail,
  onSaved,
}: {
  detail: ExperimentDetail;
  onSaved: () => void;
}) {
  const [value, setValue] = useState<ReportDetails>(() => {
    const defaults: ReportDetails = {
      lab_name: "",
      lab_address: "",
      report_number: "",
      report_date: "",
      sample_preparation: "",
      equipment: "",
      gas_and_calibration: "",
      external_steps: "",
      deviations: "",
      reviewer: "",
      repeat_group_reference: "",
    };
    return {
      ...defaults,
      ...(detail.runtime?.report_details as Partial<ReportDetails> | undefined),
    };
  });
  const [busy, setBusy] = useState(false);
  const push = useToasts((s) => s.push);
  async function save() {
    setBusy(true);
    try {
      await saveReportDetails(detail.experiment_id, value);
      push("报告补录已保存", "info");
      onSaved();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="panel p-4">
      <summary className="cursor-pointer">报告信息与人工补录</summary>
      <p className="text-[12px] my-3">
        补充实验记录，生成报告时逐项列出。留空字段显示“待补录”；补录不会更改原始采样。
      </p>
      <div className="grid grid-cols-2 gap-3">
        {fields.map(([key, label]) => (
          <label key={key} className="flex flex-col text-[12px] gap-1">
            <span>{label}</span>
            <textarea
              value={value[key]}
              maxLength={4096}
              onChange={(e) =>
                setValue((v) => ({ ...v, [key]: e.target.value }))
              }
              className="p-2 rounded"
              style={{
                background: "var(--panel-2)",
                border: "1px solid var(--border-bright)",
              }}
            />
          </label>
        ))}
      </div>
      <details className="my-3 text-[12px]">
        <summary>
          补录与计算历史（{detail.revisions?.length ?? 0} 个版本）
        </summary>
        <pre className="whitespace-pre-wrap max-h-64 overflow-auto">
          {JSON.stringify(detail.revisions ?? [], null, 2)}
        </pre>
      </details>
      <button
        disabled={busy}
        onClick={save}
        className="mt-3 px-3 py-1.5 rounded"
        style={{ background: "var(--gas)", color: "#04140f" }}
      >
        保存报告信息
      </button>
    </details>
  );
}
