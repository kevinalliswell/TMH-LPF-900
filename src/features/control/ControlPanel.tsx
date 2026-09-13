import { formatFlow } from "./formatFlow";
import { useEffect, useState } from "react";
import {
  GAS,
  GAS_NAMES,
  closeAllValves,
  getGasSafetyLimits,
  openAllValves,
  setGasFlow,
  setGasOnOff,
  shutoffFlammableGases,
  tareBalance,
} from "../../ipc";
import type { GasLimits } from "../../ipc/types";
import { useTelemetry } from "../../stores/telemetry";
import { toastError } from "../../stores/toast";

const btn = {
  background: "var(--panel-2)",
  border: "1px solid var(--border-bright)",
  color: "var(--fg)",
} as const;

export function ControlPanel() {
  const snap = useTelemetry((s) => s.snapshot);
  const [inputs, setInputs] = useState<number[]>([0, 0, 0, 0]);
  const [toast, setToast] = useState<{ msg: string; warn: boolean } | null>(null);
  const [taring, setTaring] = useState(false);
  // Display the configured safety limits (#13) — enforcement stays in the backend clamp.
  const [limits, setLimits] = useState<GasLimits | null>(null);
  useEffect(() => {
    getGasSafetyLimits()
      .then(setLimits)
      .catch(() => {});
  }, []);

  const flash = (msg: string, warn = false) => {
    setToast({ msg, warn });
    window.setTimeout(() => setToast(null), 3200);
  };

  const doSet = async (g: number) => {
    try {
      const r = await setGasFlow(g, inputs[g] ?? 0);
      if (r.clamped) flash(`${GAS_NAMES[g]}：已按安全上限 ${formatFlow(r.applied)} L/min 执行`, true);
      else flash(`${GAS_NAMES[g]} 设定为 ${formatFlow(r.applied)} L/min`);
    } catch (e) {
      toastError(e);
    }
  };

  const doTare = async () => {
    setTaring(true);
    try {
      const r = await tareBalance();
      flash(r.success ? `去皮成功（${(r.weight ?? 0).toFixed(3)} g）` : "去皮失败，请重试", !r.success);
    } catch (e) {
      toastError(e);
    } finally {
      setTaring(false);
    }
  };

  const doOnOff = async (g: number, on: boolean) => {
    try {
      await setGasOnOff(g, on);
    } catch (e) {
      toastError(e);
    }
  };

  const doValves = async (open: boolean) => {
    try {
      await (open ? openAllValves() : closeAllValves());
    } catch (e) {
      toastError(e);
    }
  };

  // Only report success once the emergency shutoff actually resolves (issue #9).
  const doShutoff = async () => {
    try {
      await shutoffFlammableGases();
      flash("已切断可燃气（H₂ / CO₂ / CO → 0）", true);
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <section className="panel p-3">
        <div className="section-title mb-2">气体控制</div>
        <div className="flex flex-col gap-2">
          {GAS_NAMES.map((name, g) => {
            const limited = g === GAS.H2 || g === GAS.CO;
            const on = snap?.gas[g]?.on ?? false;
            return (
              <div key={name} className="flex items-center gap-2">
                <span className="w-7 text-[13px] font-medium" style={{ color: "var(--gas)" }}>
                  {name}
                </span>
                <input
                  type="number"
                  step={limits?.flow_steps?.[g] ?? "any"}
                  min="0"
                  value={inputs[g] ?? 0}
                  onChange={(e) =>
                    setInputs((p) => p.map((v, i) => (i === g ? parseFloat(e.target.value) || 0 : v)))
                  }
                  className="mono w-16 px-2 py-1 rounded text-right text-[13px]"
                  style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
                />
                <button onClick={() => doSet(g)} className="px-2.5 py-1 rounded text-[12px]" style={btn}>
                  设定
                </button>
                <button
                  onClick={() => doOnOff(g, !on)}
                  className="px-2.5 py-1 rounded text-[12px] w-9"
                  style={btn}
                >
                  {on ? "关" : "开"}
                </button>
                {limited && (
                  <span className="mono text-[10px]" style={{ color: "var(--warn)" }}>
                    ≤{(g === GAS.H2 ? limits?.h2 : limits?.co) ?? 5}
                  </span>
                )}
              </div>
            );
          })}
        </div>
        <div className="flex gap-2 mt-3">
          <button onClick={() => doValves(true)} className="flex-1 py-1.5 rounded text-[12px]" style={btn}>
            全开
          </button>
          <button onClick={() => doValves(false)} className="flex-1 py-1.5 rounded text-[12px]" style={btn}>
            全关
          </button>
        </div>
        <button
          onClick={doShutoff}
          className="w-full py-2 mt-2 rounded text-[13px] font-semibold"
          style={{ background: "var(--danger)", color: "#fff", boxShadow: "0 0 14px rgba(255,69,58,0.35)" }}
        >
          ⚠ 一键切断可燃气
        </button>
      </section>

      <section className="panel p-3">
        <div className="section-title mb-2">天平</div>
        <div className="mono text-xl mb-2" style={{ color: snap?.weight == null ? "var(--fg-faint)" : "var(--fg)" }}>
          {snap?.weight == null ? "—" : `${snap.weight.toFixed(3)} g`}
        </div>
        <button
          onClick={doTare}
          disabled={taring}
          className="w-full py-2 rounded text-[13px]"
          style={{ ...btn, opacity: taring ? 0.6 : 1 }}
        >
          {taring ? "去皮中…" : "去皮 (Tare)"}
        </button>
      </section>

      {toast && (
        <div
          className="panel p-2.5 text-[12px]"
          style={{
            borderColor: toast.warn ? "var(--warn)" : "var(--border-bright)",
            color: "var(--fg)",
          }}
        >
          {toast.msg}
        </div>
      )}
    </div>
  );
}
