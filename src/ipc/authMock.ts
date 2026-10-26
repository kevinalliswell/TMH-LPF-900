import type { AuthSnapshot, PublicUser, UserRole } from "./types";

type MockUser = PublicUser & { password: string };

let users: MockUser[] = [];
let activeUserId: string | null = null;
let locked = false;

const now = () => new Date().toISOString();
const publicUser = (user: MockUser): PublicUser => ({
  user_id: user.user_id,
  username: user.username,
  display_name: user.display_name,
  role: user.role,
  is_active: user.is_active,
  must_change_password: user.must_change_password,
  created_at: user.created_at,
  last_login_at: user.last_login_at,
});

export function mockAuthState(): AuthSnapshot {
  if (users.length === 0) return { status: "SETUP_REQUIRED", user: null, idle_timeout_s: 900 };
  if (locked) return { status: "LOCKED", user: null, idle_timeout_s: 900 };
  const user = users.find((item) => item.user_id === activeUserId);
  if (!user) return { status: "ANONYMOUS", user: null, idle_timeout_s: 900 };
  return {
    status: user.must_change_password ? "PASSWORD_CHANGE_REQUIRED" : "AUTHENTICATED",
    user: publicUser(user),
    idle_timeout_s: 900,
  };
}

export function mockCurrentUser(): PublicUser | null {
  const user = users.find((item) => item.user_id === activeUserId);
  return user ? publicUser(user) : null;
}

/** Enforce the same authenticated-session contract as native user commands. */
export function mockRequireUser(): PublicUser {
  const snapshot = mockAuthState();
  if (snapshot.status === "PASSWORD_CHANGE_REQUIRED") {
    throw new Error("PASSWORD_CHANGE_REQUIRED");
  }
  if (snapshot.status !== "AUTHENTICATED" || !snapshot.user) {
    throw new Error("AUTH_REQUIRED");
  }
  return snapshot.user;
}

export function mockBootstrapAdmin(
  username: string,
  displayName: string,
  password: string,
): PublicUser {
  if (users.length) throw new Error("ALREADY_INITIALIZED");
  validate(username, displayName, password);
  const user: MockUser = {
    user_id: crypto.randomUUID(),
    username: username.trim().toLowerCase(),
    display_name: displayName.trim(),
    role: "ADMIN",
    is_active: true,
    must_change_password: false,
    created_at: now(),
    last_login_at: now(),
    password,
  };
  users = [user];
  activeUserId = user.user_id;
  return publicUser(user);
}

export function mockLogin(username: string, password: string): AuthSnapshot {
  const user = users.find((item) => item.username === username.trim().toLowerCase());
  if (!user || user.password !== password) throw new Error("AUTH_FAILED");
  if (!user.is_active) throw new Error("ACCOUNT_DISABLED");
  activeUserId = user.user_id;
  locked = false;
  user.last_login_at = now();
  return mockAuthState();
}

export function mockLogout(): void {
  activeUserId = null;
  locked = false;
}

export function mockLock(): void {
  activeUserId = null;
  locked = true;
}

export function mockChangePassword(currentPassword: string, newPassword: string): void {
  const user = users.find((item) => item.user_id === activeUserId);
  if (!user || user.password !== currentPassword) throw new Error("AUTH_FAILED");
  if (newPassword.length < 10) throw new Error("INVALID_PARAMS");
  user.password = newPassword;
  user.must_change_password = false;
}

export function mockListUsers(): PublicUser[] {
  requireAdmin();
  return users.map(publicUser);
}

export function mockCreateUser(
  username: string,
  displayName: string,
  temporaryPassword: string,
  role: UserRole,
): PublicUser {
  requireAdmin();
  validate(username, displayName, temporaryPassword);
  if (users.some((item) => item.username === username.trim().toLowerCase())) {
    throw new Error("USERNAME_EXISTS");
  }
  const user: MockUser = {
    user_id: crypto.randomUUID(),
    username: username.trim().toLowerCase(),
    display_name: displayName.trim(),
    role,
    is_active: true,
    must_change_password: true,
    created_at: now(),
    last_login_at: null,
    password: temporaryPassword,
  };
  users.push(user);
  return publicUser(user);
}

export function mockUpdateUser(
  userId: string,
  displayName: string,
  role: UserRole,
  isActive: boolean,
): PublicUser {
  const actor = requireAdmin();
  const user = users.find((item) => item.user_id === userId);
  if (!user) throw new Error("USER_NOT_FOUND");
  if (actor.user_id === userId && (role !== user.role || !isActive)) throw new Error("FORBIDDEN");
  if (user.role === "ADMIN" && user.is_active && (role !== "ADMIN" || !isActive) && users.filter((item) => item.role === "ADMIN" && item.is_active).length <= 1) throw new Error("LAST_ADMIN");
  user.display_name = displayName.trim();
  user.role = role;
  user.is_active = isActive;
  return publicUser(user);
}

export function mockResetPassword(userId: string, password: string): void {
  const actor = requireAdmin();
  if (actor.user_id === userId) throw new Error("FORBIDDEN");
  const user = users.find((item) => item.user_id === userId);
  if (!user) throw new Error("USER_NOT_FOUND");
  if (password.length < 10) throw new Error("INVALID_PARAMS");
  user.password = password;
  user.must_change_password = true;
}

function requireAdmin(): MockUser {
  const user = mockRequireUser();
  const actor = users.find((item) => item.user_id === user.user_id);
  if (!actor) throw new Error("AUTH_REQUIRED");
  if (actor.role !== "ADMIN") throw new Error("FORBIDDEN");
  return actor;
}

function validate(username: string, displayName: string, password: string): void {
  if (!/^[A-Za-z0-9._-]{3,32}$/.test(username.trim()) || !displayName.trim()) {
    throw new Error("INVALID_PARAMS");
  }
  if (password.length < 10 || password.length > 128) throw new Error("INVALID_PARAMS");
}
