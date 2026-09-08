import { useEffect, useState } from "react";
import {
  generateTestGroupReport,
  getTestGroup,
  isTauri,
  listExperiments,
  listTestGroups,
  previewTestGroup,
  saveTestGroup,
} from "../../ipc";
import type {
  ExperimentSummary,
  GroupRequest,
  GroupSnapshot,
  TestGroupRevision,
} from "../../ipc";
import { useNav } from "../../stores/nav";
import { GroupForm } from "./GroupForm";
import { GroupResults } from "./GroupResults";
const buttonClass =
  "px-3 py-2 rounded border border-[var(--border)] hover:bg-[var(--panel-2)] disabled:opacity-40 disabled:cursor-not-allowed";
function newRequest(): GroupRequest {
  return {
    group_id: null,
    expected_revision: null,
    name: "",
    batch_id: "",
    same_batch_confirmed: false,
    material: "pellet",
    manual_limits: null,
    trials: [],
    reason: "",
    report_details: {
      lab_name: "",
      lab_address: "",
      report_number: "",
      report_date: "",
      reviewer: "",
      deviations: "",
    },
  };
}
function fromRecord(record: TestGroupRevision): GroupRequest {
  const s = record.snapshot;
  return {
    group_id: record.group_id,
    expected_revision: record.revision,
    name: s.name,
    batch_id: s.batch_id,
    same_batch_confirmed: s.same_batch_confirmed,
    material: s.policy.material,
    manual_limits: s.policy.manual_limits,
    report_details: s.report_details,
    reason: "",
    trials: s.trials.map((t) => ({
      experiment_id: t.experiment.experiment_id,
      exclusion_reason: t.exclusion_reason,
    })),
  };
}
export function GroupView() {
  if (!isTauri())
    return (
      <div className="p-6 space-y-3">
        <h1 className="text-lg font-semibold">检验组与报告</h1>
        <p>
          请在桌面应用中使用检验组。此功能需要实验数据库中的单次分析、程序与运行证据。
        </p>
        <p className="text-[var(--fg-dim)]">
          桌面模拟设备可验证完整的组检与报告流程。
        </p>
      </div>
    );
  return <DesktopGroups />;
}
function DesktopGroups() {
  const [groups, setGroups] = useState<TestGroupRevision[]>([]);
  const [offset, setOffset] = useState(0);
  const [experiments, setExperiments] = useState<ExperimentSummary[]>([]);
  const [record, setRecord] = useState<TestGroupRevision | null>(null);
  const [latestRevision, setLatestRevision] = useState(0);
  const [request, setRequest] = useState(newRequest);
  const [preview, setPreview] = useState<GroupSnapshot | null>(null);
  const [busy, setBusy] = useState(true);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const setLeaveBlockReason = useNav((s) => s.setLeaveBlockReason);
  useEffect(() => {
    setLeaveBlockReason(
      busy
        ? "检验组正在处理，请等待完成"
        : dirty
          ? "检验组有未保存修改，请保存或撤销后切换"
          : null,
    );
    return () => setLeaveBlockReason(null);
  }, [busy, dirty, setLeaveBlockReason]);
  const oldVersion = !!record && record.revision < latestRevision;
  useEffect(() => {
    let active = true;
    Promise.all([listTestGroups(), listExperiments()])
      .then(([g, e]) => {
        if (active) {
          setGroups(g);
          setExperiments(e);
        }
      })
      .catch((e) => {
        if (active) setError(String(e instanceof Error ? e.message : e));
      })
      .finally(() => {
        if (active) setBusy(false);
      });
    return () => {
      active = false;
    };
  }, []);
  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }
  function load(record: TestGroupRevision, latest: number) {
    setRecord(record);
    setLatestRevision(latest);
    setRequest(fromRecord(record));
    setPreview(record.snapshot);
    setDirty(false);
  }
  function change(value: GroupRequest) {
    setRequest(value);
    setPreview(null);
    setDirty(true);
    setNotice("");
  }
  function validateLimits() {
    if (
      request.manual_limits &&
      ![
        request.manual_limits.a,
        request.manual_limits.b,
        request.manual_limits.c,
      ].every(Number.isFinite)
    )
      throw new Error("请完整填写 A、B、C 重复性限值及依据");
  }
  const canNavigate = !busy && !dirty;
  return (
    <div className="flex flex-1 min-h-0 text-[13px]">
      <aside className="w-60 shrink-0 border-r border-[var(--border)] overflow-auto p-3 space-y-3">
        <h1 className="text-base font-semibold">检验组与报告</h1>
        <button
          className={buttonClass}
          disabled={!canNavigate}
          onClick={() => {
            setRecord(null);
            setLatestRevision(0);
            setRequest(newRequest());
            setPreview(null);
            setError("");
            setNotice("");
          }}
        >
          新建检验组
        </button>
        {groups.map((g) => (
          <button
            key={g.group_id}
            className={`${buttonClass} w-full text-left ${record?.group_id === g.group_id ? "bg-[var(--panel-2)]" : ""}`}
            disabled={!canNavigate}
            onClick={() =>
              void run(async () => {
                const current = await getTestGroup(g.group_id);
                load(current, current.revision);
              })
            }
          >
            <span className="block truncate">{g.snapshot.name}</span>
            <small className="text-[var(--fg-dim)]">
              {g.snapshot.batch_id} · v{g.revision}
            </small>
          </button>
        ))}
        {!groups.length && (
          <p className="text-[var(--fg-dim)]">本页暂无检验组</p>
        )}
        <div className="flex gap-2">
          <button
            className={buttonClass}
            disabled={!canNavigate || offset === 0}
            onClick={() =>
              void run(async () => {
                const next = Math.max(0, offset - 25);
                setGroups(await listTestGroups(next));
                setOffset(next);
              })
            }
          >
            上一页
          </button>
          <button
            className={buttonClass}
            disabled={!canNavigate || groups.length < 25}
            onClick={() =>
              void run(async () => {
                const next = offset + 25;
                setGroups(await listTestGroups(next));
                setOffset(next);
              })
            }
          >
            下一页
          </button>
        </div>
        <p className="text-[11px] text-[var(--fg-dim)]">
          第 {Math.floor(offset / 25) + 1} 页，每页最多 25 组。
        </p>
      </aside>
      <main className="flex-1 min-w-0 overflow-auto p-4 space-y-4">
        {error && (
          <p role="alert" className="p-3 border border-[var(--danger)] rounded">
            {error}
          </p>
        )}
        {notice && (
          <p role="status" className="p-3 panel break-all">
            {notice}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-base font-semibold">
            {record
              ? `${record.snapshot.name} · v${record.revision}`
              : "新建检验组"}
          </h2>
          {record && (
            <>
              <button
                className={buttonClass}
                disabled={!canNavigate || record.revision <= 1}
                onClick={() =>
                  void run(async () =>
                    load(
                      await getTestGroup(record.group_id, record.revision - 1),
                      latestRevision,
                    ),
                  )
                }
              >
                较早版本
              </button>
              <button
                className={buttonClass}
                disabled={!canNavigate || record.revision >= latestRevision}
                onClick={() =>
                  void run(async () =>
                    load(
                      await getTestGroup(record.group_id, record.revision + 1),
                      latestRevision,
                    ),
                  )
                }
              >
                较新版本
              </button>
              <button
                className={buttonClass}
                disabled={busy || dirty}
                onClick={() =>
                  void run(async () =>
                    setNotice(
                      `报告已保存：${await generateTestGroupReport(record.group_id, record.revision)}；同目录保存完整 JSON 证据。`,
                    ),
                  )
                }
              >
                导出 v{record.revision} 报告
              </button>
            </>
          )}
        </div>
        {oldVersion && (
          <p>正在查看历史版本。返回最新版本后可追加修订；历史内容保持原样。</p>
        )}
        {dirty && (
          <div className="flex gap-3 items-center">
            <p>有未保存修改，请保存或撤销后再切换。</p>
            <button
              className={buttonClass}
              disabled={busy}
              onClick={() => {
                if (record) load(record, latestRevision);
                else {
                  setRequest(newRequest());
                  setPreview(null);
                  setDirty(false);
                }
                setError("");
              }}
            >
              撤销未保存修改
            </button>
          </div>
        )}
        <GroupForm
          value={request}
          change={change}
          experiments={experiments}
          record={record}
          disabled={busy || oldVersion}
        />
        {!oldVersion && (
          <div className="flex flex-wrap gap-3">
            <button
              className={buttonClass}
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  validateLimits();
                  setPreview(await previewTestGroup(request));
                })
              }
            >
              预览重复性结果
            </button>
            <button
              className={`${buttonClass} bg-[var(--gas)] text-[#04140f]`}
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  validateLimits();
                  const saved = await saveTestGroup(request);
                  load(saved, saved.revision);
                  setNotice(`已保存检验组 v${saved.revision}`);
                  setGroups(await listTestGroups(offset));
                })
              }
            >
              保存新版本
            </button>
          </div>
        )}
        {preview && <GroupResults snapshot={preview} />}
      </main>
    </div>
  );
}
