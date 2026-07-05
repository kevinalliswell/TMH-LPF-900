import { useCallback, useEffect, useState } from "react";
import {
  deleteExperiment,
  EXPERIMENT_MODES,
  getExperimentDetail,
  getExperimentSamples,
  listExperiments,
} from "../../ipc";
import type { ExperimentDetail, ExperimentSummary, SampleDto } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";
import { Modal } from "../../components/Modal";
import { AnalyzePanel } from "./AnalyzePanel";
import { HistoryChart } from "./HistoryChart";

const modeName = (id?: string | null) =>
  EXPERIMENT_MODES.find((m) => m.id === id)?.name ?? id ?? "—";
const fmtTime = (s?: string | null) => (s ? s.replace("T", " ").slice(0, 19) : "—");

function Meta({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
        {label}
      </span>
      <span className="mono text-[13px]">{value}</span>
    </div>
  );
}

export function HistoryView() {
  const [list, setList] = useState<ExperimentSummary[]>([]);
  const [selId, setSelId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ExperimentDetail | null>(null);
  const [samples, setSamples] = useState<SampleDto[]>([]);
  const [confirmDel, setConfirmDel] = useState(false);
  const push = useToasts((s) => s.push);

  useEffect(() => {
    listExperiments()
      .then((l) => {
        setList(l);
        setSelId((cur) => cur ?? l[0]?.experiment_id ?? null);
      })
      .catch(toastError);
  }, []);

  const loadDetail = useCallback((id: string) => {
    Promise.all([getExperimentDetail(id), getExperimentSamples(id, 1500)])
      .then(([d, s]) => {
        setDetail(d);
        setSamples(s);
      })
      .catch(toastError);
  }, []);

  useEffect(() => {
    if (selId) loadDetail(selId);
    else {
      setDetail(null);
      setSamples([]);
    }
  }, [selId, loadDetail]);

  const refreshList = useCallback(
    () =>
      listExperiments()
        .then(setList)
        .catch(toastError),
    [],
  );

  const doDelete = async () => {
    if (!selId) return;
    try {
      await deleteExperiment(selId);
      push("实验已删除", "warn");
      const l = await listExperiments();
      setList(l);
      setSelId(l[0]?.experiment_id ?? null);
    } catch (e) {
      toastError(e);
    } finally {
      setConfirmDel(false);
    }
  };

  return (
    <div className="flex-1 min-h-0 flex">
      <div
        className="w-[300px] shrink-0 border-r overflow-auto"
        style={{ borderColor: "var(--border)" }}
      >
        <div
          className="px-3 py-2 text-[11px] uppercase tracking-wider border-b"
          style={{ color: "var(--fg-faint)", borderColor: "var(--border)" }}
        >
          实验记录 · {list.length}
        </div>
        {list.length === 0 && (
          <div className="p-4 text-[12px]" style={{ color: "var(--fg-faint)" }}>
            暂无实验记录
          </div>
        )}
        {list.map((e) => {
          const active = e.experiment_id === selId;
          return (
            <button
              key={e.experiment_id}
              onClick={() => setSelId(e.experiment_id)}
              className="w-full text-left px-3 py-2.5 border-b block"
              style={{
                borderColor: "var(--border)",
                background: active ? "var(--panel-2)" : "transparent",
                borderLeft: active ? "2px solid var(--heat)" : "2px solid transparent",
              }}
            >
              <div className="flex items-center justify-between">
                <span className="text-[13px] font-medium truncate">{e.sample_name}</span>
                {e.has_analysis && (
                  <span
                    className="text-[10px] px-1 rounded shrink-0"
                    style={{ background: "rgba(56,214,196,0.12)", color: "var(--gas)" }}
                  >
                    已分析
                  </span>
                )}
              </div>
              <div className="text-[11px] mt-0.5" style={{ color: "var(--fg-dim)" }}>
                {modeName(e.experiment_type)}
              </div>
              <div className="mono text-[10px] mt-0.5" style={{ color: "var(--fg-faint)" }}>
                {fmtTime(e.start_time)} · {e.end_time ? "完成" : "进行中"}
              </div>
            </button>
          );
        })}
      </div>

      <div className="flex-1 min-h-0 overflow-auto p-4">
        {detail ? (
          <div className="flex flex-col gap-4">
            <div className="panel p-4">
              <div className="flex items-start justify-between">
                <div className="text-[15px] font-semibold">{detail.sample_name}</div>
                <button
                  onClick={() => setConfirmDel(true)}
                  className="px-2.5 py-1 rounded text-[12px]"
                  style={{ background: "var(--panel-2)", border: "1px solid var(--danger)", color: "var(--danger)" }}
                >
                  删除
                </button>
              </div>
              <div className="grid grid-cols-4 gap-4 mt-3">
                <Meta label="实验编号" value={detail.experiment_id} />
                <Meta label="模式" value={modeName(detail.experiment_type)} />
                <Meta label="操作员" value={detail.operator ?? "—"} />
                <Meta label="样品重量" value={`${detail.sample_weight} g`} />
                <Meta label="开始时间" value={fmtTime(detail.start_time)} />
                <Meta label="结束时间" value={fmtTime(detail.end_time)} />
                <Meta label="采样点数" value={String(detail.sample_count)} />
                <Meta label="备注" value={detail.description ?? "—"} />
              </div>
            </div>

            <div className="panel p-3">
              <div className="section-title px-1 pb-1">温度 / 质量曲线</div>
              {samples.length ? (
                <HistoryChart samples={samples} />
              ) : (
                <div className="text-[12px] p-6 text-center" style={{ color: "var(--fg-faint)" }}>
                  无采样数据
                </div>
              )}
            </div>

            <AnalyzePanel
              detail={detail}
              onAnalyzed={() => {
                if (selId) loadDetail(selId);
                void refreshList();
              }}
            />
          </div>
        ) : (
          <div className="text-[13px]" style={{ color: "var(--fg-faint)" }}>
            选择左侧一条实验查看详情
          </div>
        )}
      </div>

      {confirmDel && detail && (
        <Modal title="删除实验" onClose={() => setConfirmDel(false)}>
          <div className="text-[13px] mb-4" style={{ color: "var(--fg-dim)" }}>
            确认删除实验「{detail.sample_name}」及其全部采样数据?此操作不可撤销。
          </div>
          <div className="flex justify-end gap-2">
            <button
              onClick={() => setConfirmDel(false)}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
            >
              取消
            </button>
            <button
              onClick={doDelete}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{ background: "var(--danger)", color: "#fff" }}
            >
              确认删除
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
