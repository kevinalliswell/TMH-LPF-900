import type { GroupSnapshot } from "../../ipc";
export function GroupResults({ snapshot }: { snapshot: GroupSnapshot }) {
  const { evaluation } = snapshot;
  const selected = new Set(
    evaluation.decision.selected_indices.map(
      (i) => snapshot.valid_trial_indices[i],
    ),
  );
  return (
    <section className="panel p-4 space-y-3" aria-label="重复性结果">
      <h2 className="font-semibold">重复性结果</h2>
      <p className="text-[var(--warn)]">
        {snapshot.data_source === "real"
          ? "真实设备记录，仍需实验室复核"
          : "模拟 / 回放数据，仅用于软件测试"}
      </p>
      <p>{evaluation.explanation}</p>
      {Object.keys(evaluation.reported).length === 0 ? (
        <p>
          待补测，当前需要 {evaluation.decision.required_count} 次有效试验。
        </p>
      ) : (
        <dl className="grid grid-cols-3 gap-3">
          {Object.entries(evaluation.reported).map(([key, value]) => (
            <div key={key}>
              <dt className="text-[var(--fg-dim)]">{key}</dt>
              <dd className="text-xl mono">
                {value === null
                  ? "缺失"
                  : `${value} ${key === "RVI" ? "%/min" : "%"}`}
              </dd>
            </div>
          ))}
        </dl>
      )}
      <div className="overflow-auto">
        <table className="w-full text-left text-[12px]">
          <thead>
            <tr>
              <th>样品 / 分析版本</th>
              <th>判定用值</th>
              <th>采用或排除依据</th>
            </tr>
          </thead>
          <tbody>
            {snapshot.trials.map((t, i) => {
              const p = snapshot.valid_trial_indices.indexOf(i);
              return (
                <tr
                  key={t.experiment.experiment_id}
                  className="border-t border-[var(--border)]"
                >
                  <td className="py-2">
                    {t.experiment.sample_id ?? t.experiment.experiment_id} /{" "}
                    {t.analysis_revision_id ?? "未记录"}
                  </td>
                  <td>{p < 0 ? "—" : evaluation.decision_values[p]}</td>
                  <td>
                    {t.exclusion_reason !== null
                      ? `人工排除：${t.exclusion_reason}`
                      : selected.size === 0
                        ? "待补测"
                        : selected.has(i)
                          ? "采用"
                          : "按规则去极值"}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="text-[12px] text-[var(--fg-dim)]">
        A / B / C：{evaluation.limits.a} / {evaluation.limits.b} /{" "}
        {evaluation.limits.c} 个百分点 · {evaluation.limits.basis}
      </p>
      <p className="text-[12px]">
        {snapshot.policy.custom
          ? "自定义条件，不能据此声明符合国标方法。"
          : "全过程方法符合性尚未判定，需核对过程、设备与人工操作证据。"}{" "}
        采购性能合格性尚未判定。
      </p>
      <details className="text-[12px]">
        <summary className="cursor-pointer">本次完整证据</summary>
        <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all">
          {JSON.stringify(snapshot, null, 2)}
        </pre>
      </details>
    </section>
  );
}
