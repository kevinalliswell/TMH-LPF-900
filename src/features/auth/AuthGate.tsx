import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import {
  bootstrapAdmin,
  changeOwnPassword,
  emergencySafeShutdown,
  lockSession,
  login,
  touchSession,
} from "../../ipc";
import { useAuth } from "../../stores/auth";

export function AuthGate({ children }: { children: ReactNode }) {
  const { snapshot, loading, refresh, setSnapshot } = useAuth();

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (loading || !snapshot) return <AuthShell title="正在载入…" />;
  if (snapshot.status === "SETUP_REQUIRED") return <SetupForm onDone={refresh} />;
  if (snapshot.status === "PASSWORD_CHANGE_REQUIRED") return <PasswordChange onDone={refresh} />;
  if (snapshot.status !== "AUTHENTICATED") {
    return <LoginForm locked={snapshot.status === "LOCKED"} onDone={setSnapshot} />;
  }
  return <IdleGuard timeoutSeconds={snapshot.idle_timeout_s}>{children}</IdleGuard>;
}

function AuthShell({ title, children }: { title: string; children?: ReactNode }) {
  const [safeMessage, setSafeMessage] = useState("");
  const emergency = async () => {
    try {
      await emergencySafeShutdown();
      setSafeMessage("已执行安全停机：可燃气体关闭，氮气保护已启用。");
    } catch {
      setSafeMessage("安全停机未获设备确认，请立即按现场应急规程处置。");
    }
  };
  return (
    <main className="h-full grid place-items-center p-6">
      <div className="panel w-full max-w-[420px] p-6">
        <div className="text-xs mono" style={{ color: "var(--fg-faint)" }}>TMH-LPF-900</div>
        <h1 className="text-xl font-semibold mt-1">{title}</h1>
        {children}
        <div className="mt-6 pt-4" style={{ borderTop: "1px solid var(--border)" }}>
          <button type="button" onClick={emergency} className="w-full px-3 py-2 rounded text-sm" style={{ border: "1px solid var(--danger)", color: "var(--danger)" }}>
            紧急安全停机
          </button>
          {safeMessage && <p className="text-xs mt-2" style={{ color: "var(--fg-dim)" }}>{safeMessage}</p>}
        </div>
      </div>
    </main>
  );
}

function SetupForm({ onDone }: { onDone: () => Promise<void> }) {
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState("");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (password !== confirm) return setError("两次输入的密码不一致");
    try {
      await bootstrapAdmin(username, displayName, password);
      await onDone();
    } catch (cause) { setError(String(cause)); }
  };
  return <AuthShell title="创建首位管理员">
    <p className="text-xs mt-2" style={{ color: "var(--fg-dim)" }}>系统没有默认密码。此账户将负责创建和管理其他用户。</p>
    <form onSubmit={submit} className="flex flex-col gap-3 mt-5">
      <Field label="用户名" value={username} onChange={setUsername} autoComplete="username" />
      <Field label="显示名称" value={displayName} onChange={setDisplayName} />
      <Field label="密码（至少 10 位）" value={password} onChange={setPassword} type="password" autoComplete="new-password" />
      <Field label="确认密码" value={confirm} onChange={setConfirm} type="password" autoComplete="new-password" />
      <Submit error={error}>创建管理员</Submit>
    </form>
  </AuthShell>;
}

function LoginForm({ locked, onDone }: { locked: boolean; onDone: (value: Awaited<ReturnType<typeof login>>) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState(locked ? "会话已锁定，请重新登录" : "");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try { onDone(await login(username, password)); }
    catch (cause) { setError(authMessage(cause)); }
  };
  return <AuthShell title="用户登录">
    <form onSubmit={submit} className="flex flex-col gap-3 mt-5">
      <Field label="用户名" value={username} onChange={setUsername} autoComplete="username" />
      <Field label="密码" value={password} onChange={setPassword} type="password" autoComplete="current-password" />
      <Submit error={error}>登录</Submit>
    </form>
  </AuthShell>;
}

function PasswordChange({ onDone }: { onDone: () => Promise<void> }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState("");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (next !== confirm) return setError("两次输入的新密码不一致");
    try { await changeOwnPassword(current, next); await onDone(); }
    catch (cause) { setError(authMessage(cause)); }
  };
  return <AuthShell title="首次登录，请修改密码">
    <form onSubmit={submit} className="flex flex-col gap-3 mt-5">
      <Field label="临时密码" value={current} onChange={setCurrent} type="password" />
      <Field label="新密码（至少 10 位）" value={next} onChange={setNext} type="password" />
      <Field label="确认新密码" value={confirm} onChange={setConfirm} type="password" />
      <Submit error={error}>保存新密码</Submit>
    </form>
  </AuthShell>;
}

function IdleGuard({ timeoutSeconds, children }: { timeoutSeconds: number; children: ReactNode }) {
  const refresh = useAuth((state) => state.refresh);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastTouch = useRef(0);
  useEffect(() => {
    const activity = () => {
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => { void lockSession().finally(() => refresh()); }, timeoutSeconds * 1000);
      const now = Date.now();
      if (now - lastTouch.current > 30_000) { lastTouch.current = now; void touchSession(); }
    };
    const events = ["pointerdown", "keydown", "mousemove"] as const;
    events.forEach((name) => window.addEventListener(name, activity));
    activity();
    return () => { events.forEach((name) => window.removeEventListener(name, activity)); if (timer.current) clearTimeout(timer.current); };
  }, [refresh, timeoutSeconds]);
  return children;
}

function Field({ label, value, onChange, type = "text", autoComplete }: { label: string; value: string; onChange: (value: string) => void; type?: string; autoComplete?: string }) {
  return <label className="flex flex-col gap-1 text-xs" style={{ color: "var(--fg-dim)" }}>{label}<input required value={value} type={type} autoComplete={autoComplete} onChange={(event) => onChange(event.target.value)} className="px-3 py-2 rounded" style={{ background: "var(--bg)", border: "1px solid var(--border-bright)", color: "var(--fg)" }} /></label>;
}

function Submit({ error, children }: { error: string; children: ReactNode }) {
  return <><button className="px-3 py-2 rounded font-medium mt-1" style={{ background: "var(--heat)", color: "#1a0f06" }}>{children}</button>{error && <p role="alert" className="text-xs" style={{ color: "var(--danger)" }}>{error}</p>}</>;
}

function authMessage(cause: unknown): string {
  const value = String(cause);
  if (value.includes("ACCOUNT_LOCKED")) return "登录失败次数过多，账户已锁定 15 分钟";
  if (value.includes("ACCOUNT_DISABLED")) return "账户已停用，请联系管理员";
  if (value.includes("AUTH_FAILED")) return "用户名或密码错误";
  return value;
}
