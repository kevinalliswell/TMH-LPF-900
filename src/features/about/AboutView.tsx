import { useEffect, useState } from "react";
import { checkForUpdate, getUpdaterStatus, installUpdate } from "../../ipc";
import type { UpdateCheck } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";
import { Page, SectionTitle } from "../../components/Page";

const STANDARDS = [
  ["GB/T 13241", "铁矿石 还原性的测定", "还原度 Rt、还原速率指数"],
  ["GB/T 13242", "铁矿石 低温还原粉化的测定", "RDI +6.3 / +3.15 / −0.5"],
  ["GB/T 13240", "铁矿石 自由膨胀指数的测定", "自由膨胀指数 SI"],
];

/** Software-update card (docs/09 §9.3): manual check, offline-friendly. */
function UpdateCard() {
  const [status, setStatus] = useState<UpdateCheck | null>(null);
  const [busy, setBusy] = useState(false);
  const push = useToasts((s) => s.push);

  useEffect(() => {
    getUpdaterStatus()
      .then(setStatus)
      .catch(() => {});
  }, []);

  const check = async () => {
    setBusy(true);
    try {
      const r = await checkForUpdate();
      setStatus(r);
      if (r.available) push(`发现新版本 ${r.new_version}`, "info");
      else if (r.error) push("更新检查失败", "warn");
      else if (r.enabled) push("已是最新版本", "info");
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    setBusy(true);
    try {
      await installUpdate();
    } catch (e) {
      toastError(e);
      setBusy(false);
    }
  };

  const enabled = status?.enabled ?? false;
  return (
    <div className="panel p-5">
      <SectionTitle>软件更新</SectionTitle>
      <div className="flex items-center gap-3 flex-wrap">
        <span className="mono text-[13px]">当前版本 v{status?.current_version ?? "0.1.0"}</span>
        <span
          className="text-[11px] px-2 py-0.5 rounded"
          style={{ background: "var(--panel-2)", color: enabled ? "var(--ok)" : "var(--fg-faint)" }}
        >
          {enabled ? "更新检查已开启" : "更新检查已关闭"}
        </span>
        <div className="flex-1" />
        <button
          disabled={busy}
          onClick={check}
          className="px-3 py-1.5 rounded text-[13px]"
          style={{
            background: "var(--panel-2)",
            border: "1px solid var(--border-bright)",
            color: "var(--fg)",
            opacity: busy ? 0.6 : 1,
          }}
        >
          {busy ? "检查中…" : "检查更新"}
        </button>
      </div>

      {status?.available && (
        <div className="mt-3 rounded p-3" style={{ background: "var(--panel-2)" }}>
          <div className="text-[13px]" style={{ color: "var(--ok)" }}>
            发现新版本 v{status.new_version}
          </div>
          {status.notes && (
            <div
              className="text-[12px] mt-1 whitespace-pre-wrap"
              style={{ color: "var(--fg-dim)" }}
            >
              {status.notes}
            </div>
          )}
          <button
            disabled={busy}
            onClick={install}
            className="mt-2 px-3 py-1.5 rounded text-[13px] font-medium"
            style={{ background: "var(--heat)", color: "#1a0f06" }}
          >
            下载并安装重启
          </button>
        </div>
      )}
      {status?.error && (
        <div className="mt-3 text-[12px]" style={{ color: "var(--warn)" }}>
          {status.error}
        </div>
      )}
      <div className="mt-2 text-[11px]" style={{ color: "var(--fg-faint)" }}>
        更新检查默认关闭；配置开启后可用。离线工控机以签名安装包 + U 盘为主分发路径。
      </div>
    </div>
  );
}

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

      <UpdateCard />

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
