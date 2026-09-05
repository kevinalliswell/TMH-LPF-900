import { render, screen, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { AuthSnapshot, PublicUser } from "../../ipc/types";

const getAuthState = vi.fn<() => Promise<AuthSnapshot>>();
const lockSession = vi.fn<() => Promise<void>>();
const touchSession = vi.fn<() => Promise<void>>();

vi.mock("../../ipc", () => ({
  getAuthState: () => getAuthState(),
  lockSession: () => lockSession(),
  touchSession: () => touchSession(),
  login: vi.fn(),
  bootstrapAdmin: vi.fn(),
  changeOwnPassword: vi.fn(),
  emergencySafeShutdown: vi.fn(),
}));

const { AuthGate } = await import("./AuthGate");
const { useAuth } = await import("../../stores/auth");

const user: PublicUser = {
  user_id: "u1",
  username: "alice",
  display_name: "爱丽丝",
  role: "USER",
  is_active: true,
  must_change_password: false,
  created_at: "2026-09-01T08:00:00Z",
  last_login_at: null,
};

const snapshot = (status: AuthSnapshot["status"], idle = 900): AuthSnapshot => ({
  status,
  user: status === "AUTHENTICATED" || status === "PASSWORD_CHANGE_REQUIRED" ? user : null,
  idle_timeout_s: idle,
});

async function renderGate(state: AuthSnapshot) {
  getAuthState.mockResolvedValue(state);
  await act(async () => {
    render(
      <AuthGate>
        <div>受保护的控制台</div>
      </AuthGate>,
    );
  });
}

describe("AuthGate (#45)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    lockSession.mockResolvedValue(undefined);
    touchSession.mockResolvedValue(undefined);
    useAuth.setState({ snapshot: null, loading: true });
  });

  it("asks for the first administrator when the database has no users", async () => {
    await renderGate(snapshot("SETUP_REQUIRED"));
    expect(screen.getByText("创建首位管理员")).toBeTruthy();
    expect(screen.queryByText("受保护的控制台")).toBeNull();
  });

  it("shows the login form to an anonymous session", async () => {
    await renderGate(snapshot("ANONYMOUS"));
    expect(screen.getByText("用户登录")).toBeTruthy();
    expect(screen.queryByText("受保护的控制台")).toBeNull();
  });

  it("explains why a locked session is back at the login form", async () => {
    await renderGate(snapshot("LOCKED"));
    expect(screen.getByRole("alert").textContent).toContain("会话已锁定");
    expect(screen.queryByText("受保护的控制台")).toBeNull();
  });

  it("forces a temporary password to be replaced before anything else", async () => {
    await renderGate(snapshot("PASSWORD_CHANGE_REQUIRED"));
    expect(screen.getByText("首次登录，请修改密码")).toBeTruthy();
    expect(screen.queryByText("受保护的控制台")).toBeNull();
  });

  it("renders the application only for an authenticated session", async () => {
    await renderGate(snapshot("AUTHENTICATED"));
    expect(screen.getByText("受保护的控制台")).toBeTruthy();
  });

  it("offers emergency shutdown on every locked-out screen", async () => {
    await renderGate(snapshot("ANONYMOUS"));
    // Reaching a safe state must never require credentials (spec: 未登录仅保留紧急停机).
    expect(screen.getByRole("button", { name: "紧急安全停机" })).toBeTruthy();
  });
});

describe("AuthGate idle lock (#45)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    lockSession.mockResolvedValue(undefined);
    touchSession.mockResolvedValue(undefined);
    useAuth.setState({ snapshot: null, loading: true });
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => vi.useRealTimers());

  it("locks the session after the backend-reported idle timeout", async () => {
    await renderGate(snapshot("AUTHENTICATED", 2));
    expect(lockSession).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(2_500);
    });
    expect(lockSession).toHaveBeenCalled();
  });
});
