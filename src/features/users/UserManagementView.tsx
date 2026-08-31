import { useCallback, useEffect, useState, type FormEvent } from "react";
import { createUser, listUsers, resetUserPassword, updateUser } from "../../ipc";
import type { PublicUser, UserRole } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";

export function UserManagementView() {
  const [users, setUsers] = useState<PublicUser[]>([]);
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<UserRole>("USER");
  const push = useToasts((state) => state.push);
  const refresh = useCallback(() => listUsers().then(setUsers).catch(toastError), []);
  useEffect(() => { void refresh(); }, [refresh]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      await createUser(username, displayName, password, role);
      setUsername(""); setDisplayName(""); setPassword(""); setRole("USER");
      push("用户已创建，首次登录必须修改临时密码", "info");
      await refresh();
    } catch (cause) { toastError(cause); }
  };

  const edit = async (user: PublicUser, patch: Partial<Pick<PublicUser, "role" | "is_active">>) => {
    try {
      await updateUser(user.user_id, user.display_name, patch.role ?? user.role, patch.is_active ?? user.is_active);
      await refresh();
    } catch (cause) { toastError(cause); }
  };

  const reset = async (user: PublicUser) => {
    const temporary = window.prompt(`为 ${user.display_name} 设置临时密码（至少 10 位）`);
    if (!temporary) return;
    try { await resetUserPassword(user.user_id, temporary); push("临时密码已重置", "warn"); await refresh(); }
    catch (cause) { toastError(cause); }
  };

  return <main className="flex-1 min-h-0 overflow-auto p-5">
    <div className="panel p-4">
      <h2 className="font-semibold">创建用户</h2>
      <form onSubmit={submit} className="grid grid-cols-5 gap-3 mt-3 items-end">
        <Field label="用户名" value={username} onChange={setUsername} />
        <Field label="显示名称" value={displayName} onChange={setDisplayName} />
        <Field label="临时密码" value={password} onChange={setPassword} type="password" />
        <label className="text-xs flex flex-col gap-1" style={{ color: "var(--fg-dim)" }}>角色<select value={role} onChange={(event) => setRole(event.target.value as UserRole)} className="px-2 py-1.5 rounded" style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}><option value="USER">普通用户</option><option value="ADMIN">管理员</option></select></label>
        <button className="px-3 py-1.5 rounded text-sm" style={{ background: "var(--ok)", color: "#04140a" }}>创建</button>
      </form>
    </div>
    <div className="panel mt-4 overflow-hidden">
      <div className="px-4 py-3 text-sm font-semibold" style={{ borderBottom: "1px solid var(--border)" }}>用户列表 · {users.length}</div>
      <table className="w-full text-xs"><thead style={{ color: "var(--fg-faint)" }}><tr><th className="text-left p-3">用户</th><th className="text-left p-3">角色</th><th className="text-left p-3">状态</th><th className="text-left p-3">上次登录</th><th className="text-right p-3">操作</th></tr></thead><tbody>{users.map((user) => <tr key={user.user_id} style={{ borderTop: "1px solid var(--border)" }}><td className="p-3"><div>{user.display_name}</div><div className="mono" style={{ color: "var(--fg-faint)" }}>{user.username}</div></td><td className="p-3"><select value={user.role} onChange={(event) => void edit(user, { role: event.target.value as UserRole })} style={{ background: "var(--bg)", color: "var(--fg)" }}><option value="USER">普通用户</option><option value="ADMIN">管理员</option></select></td><td className="p-3">{user.is_active ? (user.must_change_password ? "待修改密码" : "正常") : "已停用"}</td><td className="p-3 mono">{user.last_login_at?.replace("T", " ").slice(0, 19) ?? "—"}</td><td className="p-3 text-right"><button onClick={() => void edit(user, { is_active: !user.is_active })} className="px-2 py-1">{user.is_active ? "停用" : "启用"}</button><button onClick={() => void reset(user)} className="px-2 py-1">重置密码</button></td></tr>)}</tbody></table>
    </div>
  </main>;
}

function Field({ label, value, onChange, type = "text" }: { label: string; value: string; onChange: (value: string) => void; type?: string }) {
  return <label className="text-xs flex flex-col gap-1" style={{ color: "var(--fg-dim)" }}>{label}<input required value={value} type={type} onChange={(event) => onChange(event.target.value)} className="px-2 py-1.5 rounded" style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }} /></label>;
}
