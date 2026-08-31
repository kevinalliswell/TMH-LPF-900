import { useEffect, useMemo, useState } from "react";
import {
  changeAdminPassword,
  getAdminAuthStatus,
  getCommConfig,
  getGasSafetyLimits,
  updateCommConfig,
} from "../../ipc";
import type { AdminAuthStatus, CommConfigDto, GasLimits, SerialParamsDto, TempParamsDto } from "../../ipc/types";
import { useRuntimeMode } from "../../stores/runtimeMode";
import { useExperiment } from "../../stores/experiment";
import { toastError, useToasts } from "../../stores/toast";
import { Page, SectionTitle } from "../../components/Page";

const MODE_LABEL: Record<string, string> = {
  web: "web（浏览器模拟）",
  mock: "mock（后端模拟设备）",
  real: "real（真实硬件）",
  replay: "replay（会话回放）",
};

function cloneConfig(config: CommConfigDto): CommConfigDto {
  return JSON.parse(JSON.stringify(config)) as CommConfigDto;
}

function Input({
  value,
  onChange,
  type = "text",
  disabled = false,
}: {
  value: string | number;
  onChange: (value: string) => void;
  type?: string;
  disabled?: boolean;
}) {
  return (
    <input
      type={type}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className="w-full rounded px-2 py-1.5 text-[13px]"
      style={{
        background: disabled ? "rgba(255,255,255,0.025)" : "var(--bg)",
        border: "1px solid var(--border)",
        color: disabled ? "var(--fg-dim)" : "var(--fg)",
      }}
    />
  );
}

function ReadOnly({ value }: { value: string | number }) {
  return (
    <div
      className="mono rounded px-2 py-1.5 text-[12px]"
      style={{ background: "rgba(255,255,255,0.025)", border: "1px solid var(--border)", color: "var(--fg-dim)" }}
    >
      {value}
    </div>
  );
}

function serialSummary(s: SerialParamsDto): string {
  return `${s.baudrate} ${s.bytesize}${s.parity}${s.stopbits} / ${s.timeout}s`;
}

function SerialEditor({
  title,
  value,
  onPort,
  children,
  disabled = false,
}: {
  title: string;
  value: SerialParamsDto | TempParamsDto;
  onPort: (port: string) => void;
  children?: React.ReactNode;
  disabled?: boolean;
}) {
  return (
    <section className="rounded p-3" style={{ border: "1px solid var(--border)" }}>
      <div className="mb-3 flex items-center justify-between">
        <div className="text-[13px] font-medium">{title}</div>
        <div className="mono text-[11px]" style={{ color: "var(--fg-faint)" }}>
          {serialSummary(value)}
        </div>
      </div>
      <div className="grid grid-cols-[76px_1fr_88px_1fr] gap-x-3 gap-y-2 text-[12px]">
        <label className="self-center" style={{ color: "var(--fg-dim)" }}>
          串口
        </label>
        <Input value={value.port} onChange={onPort} disabled={disabled} />
        <label className="self-center" style={{ color: "var(--fg-dim)" }}>
          参数
        </label>
        <ReadOnly value={serialSummary(value)} />
        {children}
      </div>
    </section>
  );
}

export function SettingsView() {
  const [limits, setLimits] = useState<GasLimits | null>(null);
  const [config, setConfig] = useState<CommConfigDto | null>(null);
  const [draft, setDraft] = useState<CommConfigDto | null>(null);
  const [auth, setAuth] = useState<AdminAuthStatus | null>(null);
  const [password, setPassword] = useState("");
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const mode = useRuntimeMode();
  const running = useExperiment((s) => s.state.running);
  const push = useToasts((s) => s.push);

  const load = async () => {
    const [cfg, status, gasLimits] = await Promise.all([
      getCommConfig(),
      getAdminAuthStatus(),
      getGasSafetyLimits(),
    ]);
    setConfig(cfg);
    setDraft(cloneConfig(cfg));
    setAuth(status);
    setLimits(gasLimits);
  };

  useEffect(() => {
    load().catch(toastError);
  }, []);

  const dirty = useMemo(
    () => !!config && !!draft && JSON.stringify(config) !== JSON.stringify(draft),
    [config, draft],
  );

  const patch = (mutate: (next: CommConfigDto) => void) => {
    if (!draft) return;
    const next = cloneConfig(draft);
    mutate(next);
    setDraft(next);
  };

  const save = async () => {
    if (!draft || auth?.must_change) return;
    setBusy(true);
    try {
      const saved = await updateCommConfig(draft, password);
      setConfig(saved);
      setDraft(cloneConfig(saved));
      setPassword("");
      push("通讯配置已保存，重启应用后生效", "info");
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  const changePassword = async () => {
    if (newPassword !== confirmPassword) {
      push("两次输入的新口令不一致", "warn");
      return;
    }
    setBusy(true);
    try {
      await changeAdminPassword(oldPassword, newPassword);
      setOldPassword("");
      setNewPassword("");
      setConfirmPassword("");
      setAuth(await getAdminAuthStatus());
      push("管理员口令已更新", "info");
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  if (!draft) {
    return (
      <Page>
        <div className="panel p-4 text-[13px]" style={{ color: "var(--fg-faint)" }}>
          正在加载通讯配置...
        </div>
      </Page>
    );
  }

  return (
    <Page>
      <div className="text-[13px]" style={{ color: "var(--fg-dim)" }}>
        通讯设置用于修改串口和设备地址；实验运行中只读，保存后需重启应用生效。
      </div>

      <div className="panel p-4">
        <div className="flex items-center justify-between">
          <SectionTitle>管理员口令</SectionTitle>
          {auth?.must_change && (
            <span className="text-[12px]" style={{ color: "var(--warn)" }}>
              当前为默认口令 1952，请先修改
            </span>
          )}
        </div>
        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
            当前口令
            <Input value={oldPassword} type="password" onChange={setOldPassword} />
          </label>
          <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
            新口令
            <Input value={newPassword} type="password" onChange={setNewPassword} />
          </label>
          <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
            确认新口令
            <div className="flex gap-2">
              <Input value={confirmPassword} type="password" onChange={setConfirmPassword} />
              <button
                type="button"
                disabled={busy}
                onClick={changePassword}
                className="shrink-0 rounded px-3 py-1.5 text-[12px]"
                style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
              >
                修改
              </button>
            </div>
          </label>
        </div>
      </div>

      <div className="panel p-4">
        <SectionTitle>串口与设备地址</SectionTitle>
        <div className="flex flex-col gap-3">
          <SerialEditor
            title="温度控制器"
            value={draft.COM_RS485_TEMP}
            onPort={(port) => patch((next) => (next.COM_RS485_TEMP.port = port))}
            disabled={running}
          >
            <label className="self-center" style={{ color: "var(--fg-dim)" }}>
              地址
            </label>
            <Input
              type="number"
              value={draft.COM_RS485_TEMP.slave_address}
              disabled={running}
              onChange={(v) => patch((next) => (next.COM_RS485_TEMP.slave_address = Number(v)))}
            />
          </SerialEditor>

          <SerialEditor
            title="气体流量 MFC"
            value={draft.COM_RS485_MFC}
            onPort={(port) => patch((next) => (next.COM_RS485_MFC.port = port))}
            disabled={running}
          >
            {(["H2", "N2", "CO2", "CO"] as const).map((gas) => (
              <label key={gas} className="contents">
                <span className="self-center" style={{ color: "var(--fg-dim)" }}>
                  {gas} 地址
                </span>
                <Input
                  type="number"
                  value={draft.SLAVE_ADDRESS_MFC[gas]}
                  disabled={running}
                  onChange={(v) => patch((next) => (next.SLAVE_ADDRESS_MFC[gas] = Number(v)))}
                />
              </label>
            ))}
          </SerialEditor>

          <SerialEditor
            title="电子天平"
            value={draft.COM_RS232_Balance}
            onPort={(port) => patch((next) => (next.COM_RS232_Balance.port = port))}
            disabled={running}
          />
        </div>

        <div className="mt-4 grid grid-cols-[1fr_180px_120px] items-end gap-3">
          <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
            保存口令
            <Input value={password} type="password" onChange={setPassword} />
          </label>
          <button
            type="button"
            onClick={() => config && setDraft(cloneConfig(config))}
            disabled={!dirty || busy}
            className="rounded px-3 py-2 text-[12px]"
            style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)", opacity: !dirty || busy ? 0.5 : 1 }}
          >
            重置修改
          </button>
          <button
            type="button"
            onClick={save}
            disabled={!dirty || !password || busy || running || !!auth?.must_change}
            className="rounded px-3 py-2 text-[12px] font-medium"
            style={{ background: "var(--ok)", color: "#04140a", opacity: !dirty || !password || busy || running || auth?.must_change ? 0.55 : 1 }}
          >
            保存配置
          </button>
        </div>
      </div>

      <div className="panel p-4">
        <SectionTitle>安全与运行</SectionTitle>
        <div className="grid grid-cols-4 gap-4">
          <div className="flex flex-col">
            <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
              H₂ 上限
            </span>
            <span className="mono text-[18px]" style={{ color: "var(--danger)" }}>
              {limits ? `${limits.h2} L/min` : "--"}
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
              CO 上限
            </span>
            <span className="mono text-[18px]" style={{ color: "var(--danger)" }}>
              {limits ? `${limits.co} L/min` : "--"}
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
              设备模式
            </span>
            <span className="mono text-[13px]" style={{ color: "var(--fg-dim)" }}>
              {mode === null ? "--" : (MODE_LABEL[mode] ?? mode)}
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
              保存状态
            </span>
            <span className="mono text-[13px]" style={{ color: running ? "var(--warn)" : "var(--fg-dim)" }}>
              {running ? "实验运行中只读" : "可编辑"}
            </span>
          </div>
        </div>
      </div>
    </Page>
  );
}
