import { useEffect, useRef, useState, type FormEvent, type CSSProperties } from "react";
import { getCommConfig, saveCommConfig, restartForConfig } from "../../ipc";
import type { CommConfig, ConfigSnapshot, GasKey, SerialConfig } from "../../ipc";
import { useAuth } from "../../stores/auth";
import { useExperiment } from "../../stores/experiment";
import { useNav } from "../../stores/nav";
import { useRuntimeMode } from "../../stores/runtimeMode";
import { Page, SectionTitle } from "../../components/Page";

const inputStyle: CSSProperties = { background: "var(--bg)", color: "var(--fg)", border: "1px solid var(--border)" };
const buttonClass = "px-3 py-2 rounded text-sm border disabled:opacity-40";
const buses = [["COM_RS485_TEMP", "温度控制器"], ["COM_RS485_MFC", "气体流量 MFC"], ["COM_RS232_Balance", "电子天平"]] as const;
const gases: GasKey[] = ["H2", "N2", "CO2", "CO"];

export function SettingsView() {
  const [snapshot, setSnapshot] = useState<ConfigSnapshot | null>(null);
  const [draft, setDraft] = useState<CommConfig | null>(null);
  const [reason, setReason] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmSave, setConfirmSave] = useState(false);
  const [confirmRestart, setConfirmRestart] = useState(false);
  const mounted = useRef(true);
  const admin = useAuth((state) => state.snapshot?.user?.role === "ADMIN");
  const experiment = useExperiment((state) => state.state);
  const mode = useRuntimeMode();
  const blocked = experiment.running || experiment.persistence_pending || experiment.updating;
  const dirty = snapshot !== null && JSON.stringify(snapshot.saved) !== JSON.stringify(draft);
  const adopt = (value: ConfigSnapshot) => { setSnapshot(value); setDraft(structuredClone(value.saved)); };

  useEffect(() => {
    mounted.current = true;
    getCommConfig().then((value) => { if (mounted.current) adopt(value); })
      .catch((cause) => { if (mounted.current) setError(String(cause)); });
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    useNav.getState().setLeaveBlockReason(busy ? "通讯配置正在处理，请稍候" : dirty ? "通讯配置尚未保存，请保存或撤销修改" : null);
    return () => useNav.getState().setLeaveBlockReason(null);
  }, [busy, dirty]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!snapshot || !draft || !admin || blocked || busy || !dirty) return;
    setConfirmSave(true);
  };
  const save = async () => {
    if (!snapshot || !draft || !admin || blocked || busy || !dirty || !confirmSave) return;
    setBusy(true); setError(""); setConfirmRestart(false);
    setConfirmSave(false);
    try {
      const result = await saveCommConfig(snapshot.revision, draft, reason.trim());
      if (mounted.current) { adopt(result); setReason(""); }
    } catch (cause) {
      if (!mounted.current) return;
      setError(String(cause));
      // A successful file replacement can be followed by an fsync failure. Re-read
      // the authoritative saved state without calling the still-running values applied.
      try {
        const actual = await getCommConfig();
        if (mounted.current && actual.revision !== snapshot.revision) adopt(actual);
      } catch (refreshError) {
        if (mounted.current) { setSnapshot(null); setDraft(null); setError(`${String(cause)}；重新读取失败：${String(refreshError)}`); }
      }
    } finally { if (mounted.current) setBusy(false); }
  };
  const restart = async () => {
    if (!snapshot || busy || dirty || blocked || !admin) return;
    setBusy(true); setError("");
    try { await restartForConfig(snapshot.revision); }
    catch (cause) { if (mounted.current) { setError(String(cause)); setBusy(false); setConfirmRestart(false); } }
  };

  return <Page wide>
    <p className="text-sm" style={{color: "var(--fg-dim)"}}>通讯参数保存到当前数据目录的配置文件。保存后重启应用才会生效。当前设备模式：{mode ?? "载入中"}。</p>
    {error && <p role="alert" className="panel p-3 text-sm" style={{color: "var(--danger)"}}>{error}</p>}
    {snapshot && draft ? <>
      <div className="panel p-4 text-sm">
        <SectionTitle>当前运行与保存状态</SectionTitle>
        <p>{snapshot.restart_required ? "已保存，重启后生效；当前不能开始新实验。" : "当前运行配置与保存内容一致。"}</p>
        <p className="mt-2">当前实验采样：{snapshot.applied.SAMPLING.interval_s} s；遥测刷新：{snapshot.applied.PERFORMANCE_CONFIG.data_collection_interval} s</p>
        <p>当前安全上限：H₂ {Math.min(snapshot.applied.GAS_SAFETY_LIMITS.H2, 5)} L/min；CO {Math.min(snapshot.applied.GAS_SAFETY_LIMITS.CO, 5)} L/min</p>
        <p className="mono text-xs break-all mt-2">配置文件：{snapshot.path}</p>
        {!admin && <p className="mt-2">普通用户可查看配置；修改和重启需要管理员。</p>}
        {blocked && <p className="mt-2">实验、数据保存或应用更新进行中，暂不能修改配置或重启。</p>}
      </div>
      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-4">
        <fieldset disabled={!admin || Boolean(blocked) || busy || confirmSave} className="flex flex-col gap-4 disabled:opacity-70">
          <legend className="text-sm mb-3">已保存配置（修改后请保存）</legend>
          {buses.map(([key, title]) => <div className="panel p-4" key={key}>
            <SectionTitle>{title}</SectionTitle>
            <SerialFields title={title} value={draft[key]} applied={snapshot.applied[key]} onChange={(patch) => setDraft({...draft, [key]: {...draft[key], ...patch}})} />
          </div>)}
          <div className="panel p-4">
            <SectionTitle>温度读值换算</SectionTitle>
            <div className="grid grid-cols-3 gap-4">
              <NumberField label="温度从站地址" value={draft.COM_RS485_TEMP.slave_address} min={0} max={247} integer onChange={(slave_address) => setDraft({...draft, COM_RS485_TEMP: {...draft.COM_RS485_TEMP, slave_address}})} />
              <NumberField label="温度换算系数" value={draft.COM_RS485_TEMP.scale} onChange={(scale) => setDraft({...draft, COM_RS485_TEMP: {...draft.COM_RS485_TEMP, scale}})} />
              <label className="text-sm self-center"><input type="checkbox" checked={draft.COM_RS485_TEMP.signed_registers} onChange={(event) => setDraft({...draft, COM_RS485_TEMP: {...draft.COM_RS485_TEMP, signed_registers: event.target.checked}})} /> 温度寄存器按有符号数读取</label>
            </div>
            <p className="text-xs mt-3" style={{color: "var(--fg-dim)"}}>当前支持寄存器起点0、T1–T9顺序的九通道读值。温控程序由仪表执行；仪表程序参数写入仍待具体型号协议接入。</p>
            <p className="mono text-xs mt-2">当前生效：地址 {snapshot.applied.COM_RS485_TEMP.slave_address} · 换算系数 {snapshot.applied.COM_RS485_TEMP.scale} · {snapshot.applied.COM_RS485_TEMP.signed_registers ? "有符号" : "无符号"}</p>
          </div>
          <div className="panel p-4">
            <SectionTitle>气体地址与流量换算</SectionTitle>
            <div className="grid grid-cols-4 gap-4">{gases.map((gas) => <div key={gas} className="flex flex-col gap-3">
              <NumberField label={`${gas} 从站地址`} value={draft.SLAVE_ADDRESS_MFC[gas]} min={1} max={99} integer onChange={(value) => setDraft({...draft, SLAVE_ADDRESS_MFC: {...draft.SLAVE_ADDRESS_MFC, [gas]: value}})} />
              <NumberField label={`${gas} 流量换算系数`} value={draft.FLOW_SCALING[gas]} onChange={(value) => setDraft({...draft, FLOW_SCALING: {...draft.FLOW_SCALING, [gas]: value}})} />
              <p className="mono text-xs">当前：地址 {snapshot.applied.SLAVE_ADDRESS_MFC[gas]} · 系数 {snapshot.applied.FLOW_SCALING[gas]}</p>
            </div>)}</div>
            <p className="text-xs mt-3" style={{color: "var(--fg-dim)"}}>流量 (L/min) = 协议原始值 ÷ 10 × 换算系数。请依据厂家资料填写；软件会检查固定氮气保护流量能否准确表示。</p>
          </div>
          <div className="panel p-4">
            <SectionTitle>安全上限与采集间隔</SectionTitle>
            <div className="grid grid-cols-3 gap-4">
              {(["H2", "CO"] as const).map((gas) => <NumberField key={gas} label={`${gas} 安全上限 (L/min)`} value={draft.GAS_SAFETY_LIMITS[gas]} max={5} onChange={(value) => setDraft({...draft, GAS_SAFETY_LIMITS: {...draft.GAS_SAFETY_LIMITS, [gas]: value}})} />)}
              <NumberField label="实验采样间隔 (s)" value={draft.SAMPLING.interval_s} min={0.01} max={86400} onChange={(interval_s) => setDraft({...draft, SAMPLING: {interval_s}, PERFORMANCE_CONFIG: {...draft.PERFORMANCE_CONFIG, experiment_data_interval: interval_s}})} />
              <NumberField label="遥测刷新间隔 (s)" value={draft.PERFORMANCE_CONFIG.data_collection_interval} min={0.01} max={86400} onChange={(value) => setDraft({...draft, PERFORMANCE_CONFIG: {...draft.PERFORMANCE_CONFIG, data_collection_interval: value}})} />
              <NumberField label="数据陈旧判定 (s)" value={draft.PERFORMANCE_CONFIG.heartbeat_timeout} min={0.01} max={86400} onChange={(value) => setDraft({...draft, PERFORMANCE_CONFIG: {...draft.PERFORMANCE_CONFIG, heartbeat_timeout: value}})} />
            </div>
          </div>
          <div className="panel p-4 flex flex-col gap-3">
            <label className="text-sm flex flex-col gap-2">修改原因<textarea required maxLength={500} value={reason} onChange={(event) => setReason(event.target.value)} className="p-2 rounded" style={inputStyle} /></label>
            <div className="flex gap-3">
              <button className={buttonClass} disabled={!dirty || !reason.trim()}>保存配置</button>
              {snapshot.restart_required && <button type="button" className={buttonClass} onClick={() => setDraft(structuredClone(snapshot.applied))}>改回当前生效配置</button>}
            </div>
          </div>
        </fieldset>
        <button type="button" className={`${buttonClass} self-start`} disabled={!dirty || busy} onClick={() => { setDraft(structuredClone(snapshot.saved)); setReason(""); setError(""); }}>撤销未保存修改</button>
      </form>
      {confirmSave && <div role="group" aria-labelledby="config-save-confirm-title" className="panel p-4 text-sm">
        <SectionTitle><span id="config-save-confirm-title">确认通讯配置变更</span></SectionTitle>
        <p>将按所填原因保存当前串口、地址、换算、安全上限和采集节奏。新配置需重启应用后生效，重启前不能开始新实验。</p>
        <p className="mt-2">修改原因：{reason.trim()}</p>
        <div className="flex gap-3 mt-3">
          <button type="button" disabled={busy || Boolean(blocked)} className={buttonClass} onClick={() => void save()}>确认保存配置</button>
          <button type="button" disabled={busy} className={buttonClass} onClick={() => setConfirmSave(false)}>取消保存</button>
        </div>
      </div>}
      {snapshot.restart_required && admin && <div className="panel p-4 text-sm">
        <p>重启前将请求保护气氛并等待设备确认，同时核对配置文件和待保存数据。重启后需要重新登录。</p>
        {confirmRestart ? <div className="flex gap-3 mt-3">
          <button type="button" disabled={busy || dirty || Boolean(blocked)} className={buttonClass} onClick={() => void restart()}>确认保护并重启</button>
          <button type="button" disabled={busy} className={buttonClass} onClick={() => setConfirmRestart(false)}>取消重启</button>
        </div> : <button type="button" disabled={busy || dirty || Boolean(blocked)} className={`${buttonClass} mt-3`} onClick={() => setConfirmRestart(true)}>重启应用以生效</button>}
      </div>}
    </> : !error && <p>正在读取配置…</p>}
  </Page>;
}

function SerialFields({title, value, applied, onChange}: {title: string; value: SerialConfig; applied: SerialConfig; onChange: (patch: Partial<SerialConfig>) => void}) {
  return <>
    <div className="grid grid-cols-3 gap-4">
      <label className="text-xs flex flex-col gap-1">{title}串口<input required value={value.port} onChange={(event) => onChange({port: event.target.value})} className="p-2 rounded" style={inputStyle} /></label>
      <NumberField label={`${title}波特率`} value={value.baudrate} min={1} max={4000000} integer onChange={(baudrate) => onChange({baudrate})} />
      <NumberField label={`${title}超时 (s)`} value={value.timeout} min={0.001} max={60} onChange={(timeout) => onChange({timeout})} />
      <NumberField label={`${title}数据位`} value={value.bytesize} min={5} max={8} integer onChange={(bytesize) => onChange({bytesize})} />
      <label className="text-xs flex flex-col gap-1">{title}校验<select value={value.parity} onChange={(event) => onChange({parity: event.target.value})} className="p-2 rounded" style={inputStyle}>{["N", "E", "O"].map((parity) => <option key={parity}>{parity}</option>)}</select></label>
      <NumberField label={`${title}停止位`} value={value.stopbits} min={1} max={2} integer onChange={(stopbits) => onChange({stopbits})} />
    </div>
    <p className="mono text-xs mt-3" style={{color: "var(--fg-dim)"}}>当前生效：{applied.port} · {applied.baudrate} · {applied.bytesize}{applied.parity}{applied.stopbits} · 超时 {applied.timeout} s</p>
  </>;
}
function NumberField({label, value, onChange, min = 0.000000001, max, integer = false}: {label: string; value: number; onChange: (value: number) => void; min?: number; max?: number; integer?: boolean}) {
  return <label className="text-xs flex flex-col gap-1">{label}<input required type="number" value={Number.isFinite(value) ? value : ""} min={min} max={max} step={integer ? 1 : "any"} onChange={(event) => onChange(event.target.value === "" ? Number.NaN : Number(event.target.value))} className="p-2 rounded" style={inputStyle} /></label>;
}
