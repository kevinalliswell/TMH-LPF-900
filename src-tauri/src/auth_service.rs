use crate::auth_session::{AuthSession, SessionError, SessionState};
use chrono::{DateTime, SecondsFormat, Utc};
use std::sync::{Mutex, MutexGuard};
use tmh_core::auth::{
    hash_password, new_user_id, normalize_username, validate_display_name, verify_password,
    UserRole,
};
use tmh_core::storage::{Storage, UserRow};

const MAX_LOGIN_FAILURES: i64 = 5;
const ACCOUNT_LOCK_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthCode {
    Required,
    Failed,
    AccountLocked,
    AccountDisabled,
    SessionExpired,
    PasswordChangeRequired,
    Forbidden,
    AlreadyInitialized,
    UsernameExists,
    InvalidInput,
    LastAdmin,
    Storage,
}

impl AuthCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "AUTH_REQUIRED",
            Self::Failed => "AUTH_FAILED",
            Self::AccountLocked => "ACCOUNT_LOCKED",
            Self::AccountDisabled => "ACCOUNT_DISABLED",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::PasswordChangeRequired => "PASSWORD_CHANGE_REQUIRED",
            Self::Forbidden => "FORBIDDEN",
            Self::AlreadyInitialized => "ALREADY_INITIALIZED",
            Self::UsernameExists => "USERNAME_EXISTS",
            Self::InvalidInput => "INVALID_PARAMS",
            Self::LastAdmin => "LAST_ADMIN",
            Self::Storage => "STORAGE_ERROR",
        }
    }
}

impl std::fmt::Display for AuthCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PublicUser {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    pub is_active: bool,
    pub must_change_password: bool,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

impl From<&UserRow> for PublicUser {
    fn from(user: &UserRow) -> Self {
        Self {
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            role: user.role,
            is_active: user.is_active,
            must_change_password: user.must_change_password,
            created_at: user.created_at.clone(),
            last_login_at: user.last_login_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthStatus {
    SetupRequired,
    Anonymous,
    Authenticated,
    PasswordChangeRequired,
    Locked,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthSnapshot {
    pub status: AuthStatus,
    pub user: Option<PublicUser>,
    pub idle_timeout_s: i64,
}

pub struct AuthService {
    session: Mutex<AuthSession>,
    idle_timeout_s: i64,
}

impl AuthService {
    pub fn new(idle_timeout_s: i64) -> Self {
        Self {
            session: Mutex::new(AuthSession::new(idle_timeout_s)),
            idle_timeout_s,
        }
    }

    pub fn bootstrap(
        &self,
        storage: &Storage,
        username: &str,
        display_name: &str,
        password: &str,
        now_epoch_s: i64,
    ) -> Result<UserRow, AuthCode> {
        if storage.user_count().map_err(|_| AuthCode::Storage)? != 0 {
            return Err(AuthCode::AlreadyInitialized);
        }
        let user = build_user(
            username,
            display_name,
            password,
            UserRole::Admin,
            false,
            now_epoch_s,
        )?;
        storage.insert_user(&user).map_err(|_| AuthCode::Storage)?;
        self.session().login(user.user_id.clone(), now_epoch_s);
        Ok(user)
    }

    pub fn login(
        &self,
        storage: &Storage,
        username: &str,
        password: &str,
        now_epoch_s: i64,
    ) -> Result<UserRow, AuthCode> {
        let username = normalize_username(username).map_err(|_| AuthCode::Failed)?;
        let Some(user) = storage
            .get_user_by_username(&username)
            .map_err(|_| AuthCode::Storage)?
        else {
            return Err(AuthCode::Failed);
        };
        if !user.is_active {
            return Err(AuthCode::AccountDisabled);
        }
        if user
            .locked_until
            .as_deref()
            .and_then(parse_timestamp)
            .is_some_and(|until| now_epoch_s < until)
        {
            return Err(AuthCode::AccountLocked);
        }
        if !verify_password(&user.password_hash, password) {
            let failed_count = user.failed_login_count.saturating_add(1);
            let locked_until = (failed_count >= MAX_LOGIN_FAILURES)
                .then(|| timestamp(now_epoch_s.saturating_add(ACCOUNT_LOCK_SECONDS)));
            storage
                .record_login_failure(&user.user_id, failed_count, locked_until.as_deref())
                .map_err(|_| AuthCode::Storage)?;
            return Err(if locked_until.is_some() {
                AuthCode::AccountLocked
            } else {
                AuthCode::Failed
            });
        }
        let now = timestamp(now_epoch_s);
        storage
            .record_login_success(&user.user_id, &now)
            .map_err(|_| AuthCode::Storage)?;
        self.session().login(user.user_id.clone(), now_epoch_s);
        storage
            .get_user(&user.user_id)
            .map_err(|_| AuthCode::Storage)?
            .ok_or(AuthCode::Failed)
    }

    pub fn logout(&self) {
        self.session().logout();
    }

    pub fn lock(&self) {
        self.session().lock();
    }

    pub fn touch(&self, now_epoch_s: i64) -> Result<(), AuthCode> {
        self.session().touch(now_epoch_s).map_err(map_session_error)
    }

    pub fn has_active_session(&self, now_epoch_s: i64) -> bool {
        matches!(self.session().state(now_epoch_s), SessionState::Active(_))
    }

    pub fn snapshot(&self, storage: &Storage, now_epoch_s: i64) -> Result<AuthSnapshot, AuthCode> {
        if storage.user_count().map_err(|_| AuthCode::Storage)? == 0 {
            return Ok(AuthSnapshot {
                status: AuthStatus::SetupRequired,
                user: None,
                idle_timeout_s: self.idle_timeout_s,
            });
        }
        let state = self.session().state(now_epoch_s);
        let (status, user) = match state {
            SessionState::Anonymous => (AuthStatus::Anonymous, None),
            SessionState::Locked => (AuthStatus::Locked, None),
            SessionState::Active(user_id) => {
                let Some(user) = storage.get_user(&user_id).map_err(|_| AuthCode::Storage)? else {
                    self.logout();
                    return Ok(AuthSnapshot {
                        status: AuthStatus::Anonymous,
                        user: None,
                        idle_timeout_s: self.idle_timeout_s,
                    });
                };
                if !user.is_active {
                    self.logout();
                    (AuthStatus::Anonymous, None)
                } else {
                    let status = if user.must_change_password {
                        AuthStatus::PasswordChangeRequired
                    } else {
                        AuthStatus::Authenticated
                    };
                    (status, Some(PublicUser::from(&user)))
                }
            }
        };
        Ok(AuthSnapshot {
            status,
            user,
            idle_timeout_s: self.idle_timeout_s,
        })
    }

    pub fn require_user(&self, storage: &Storage, now_epoch_s: i64) -> Result<UserRow, AuthCode> {
        self.require(storage, now_epoch_s, false)
    }

    pub fn require_admin(&self, storage: &Storage, now_epoch_s: i64) -> Result<UserRow, AuthCode> {
        let user = self.require_user(storage, now_epoch_s)?;
        if !user.role.is_admin() {
            return Err(AuthCode::Forbidden);
        }
        Ok(user)
    }

    pub fn create_user(
        &self,
        storage: &Storage,
        username: &str,
        display_name: &str,
        password: &str,
        role: UserRole,
        now_epoch_s: i64,
    ) -> Result<UserRow, AuthCode> {
        self.require_admin(storage, now_epoch_s)?;
        let normalized = normalize_username(username).map_err(|_| AuthCode::InvalidInput)?;
        if storage
            .get_user_by_username(&normalized)
            .map_err(|_| AuthCode::Storage)?
            .is_some()
        {
            return Err(AuthCode::UsernameExists);
        }
        let user = build_user(&normalized, display_name, password, role, true, now_epoch_s)?;
        storage.insert_user(&user).map_err(|_| AuthCode::Storage)?;
        Ok(user)
    }

    pub fn change_own_password(
        &self,
        storage: &Storage,
        current_password: &str,
        new_password: &str,
        now_epoch_s: i64,
    ) -> Result<(), AuthCode> {
        let user = self.require(storage, now_epoch_s, true)?;
        if !verify_password(&user.password_hash, current_password) {
            return Err(AuthCode::Failed);
        }
        let hash = hash_password(new_password).map_err(|_| AuthCode::InvalidInput)?;
        storage
            .set_user_password(&user.user_id, &hash, false, &timestamp(now_epoch_s))
            .map_err(|_| AuthCode::Storage)
    }

    fn require(
        &self,
        storage: &Storage,
        now_epoch_s: i64,
        allow_password_change: bool,
    ) -> Result<UserRow, AuthCode> {
        let user_id = self
            .session()
            .require(now_epoch_s)
            .map_err(map_session_error)?;
        let user = storage
            .get_user(&user_id)
            .map_err(|_| AuthCode::Storage)?
            .ok_or(AuthCode::Required)?;
        if !user.is_active {
            self.logout();
            return Err(AuthCode::AccountDisabled);
        }
        if user.must_change_password && !allow_password_change {
            return Err(AuthCode::PasswordChangeRequired);
        }
        self.session()
            .touch(now_epoch_s)
            .map_err(map_session_error)?;
        Ok(user)
    }

    fn session(&self) -> MutexGuard<'_, AuthSession> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn build_user(
    username: &str,
    display_name: &str,
    password: &str,
    role: UserRole,
    must_change_password: bool,
    now_epoch_s: i64,
) -> Result<UserRow, AuthCode> {
    let now = timestamp(now_epoch_s);
    Ok(UserRow {
        user_id: new_user_id(),
        username: normalize_username(username).map_err(|_| AuthCode::InvalidInput)?,
        display_name: validate_display_name(display_name).map_err(|_| AuthCode::InvalidInput)?,
        password_hash: hash_password(password).map_err(|_| AuthCode::InvalidInput)?,
        role,
        is_active: true,
        must_change_password,
        failed_login_count: 0,
        locked_until: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        password_changed_at: now,
        last_login_at: None,
    })
}

fn timestamp(epoch_s: i64) -> String {
    DateTime::<Utc>::from_timestamp(epoch_s, 0)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp())
}

fn map_session_error(error: SessionError) -> AuthCode {
    match error {
        SessionError::Required => AuthCode::Required,
        SessionError::Expired => AuthCode::SessionExpired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmh_core::storage::Storage;

    #[test]
    fn bootstrap_is_single_use_and_logs_in_admin() {
        let storage = Storage::open_in_memory().unwrap();
        let auth = AuthService::new(900);
        let admin = auth
            .bootstrap(&storage, "Admin", "管理员", "strong-pass", 100)
            .unwrap();
        assert_eq!(admin.username, "admin");
        assert_eq!(admin.role, UserRole::Admin);
        assert_eq!(
            auth.require_admin(&storage, 101).unwrap().user_id,
            admin.user_id
        );
        assert_eq!(
            auth.bootstrap(&storage, "other", "其他", "strong-pass", 102)
                .unwrap_err(),
            AuthCode::AlreadyInitialized
        );
    }

    #[test]
    fn fifth_bad_password_locks_account_for_fifteen_minutes() {
        let storage = Storage::open_in_memory().unwrap();
        let auth = AuthService::new(900);
        auth.bootstrap(&storage, "admin", "管理员", "strong-pass", 100)
            .unwrap();
        auth.logout();
        for now in 200..204 {
            assert_eq!(
                auth.login(&storage, "admin", "wrong-pass", now)
                    .unwrap_err(),
                AuthCode::Failed
            );
        }
        assert_eq!(
            auth.login(&storage, "admin", "wrong-pass", 204)
                .unwrap_err(),
            AuthCode::AccountLocked
        );
        assert_eq!(
            auth.login(&storage, "admin", "strong-pass", 1000)
                .unwrap_err(),
            AuthCode::AccountLocked
        );
        auth.login(&storage, "admin", "strong-pass", 1105).unwrap();
    }

    #[test]
    fn temporary_password_requires_change_before_normal_commands() {
        let storage = Storage::open_in_memory().unwrap();
        let auth = AuthService::new(900);
        auth.bootstrap(&storage, "admin", "管理员", "strong-pass", 100)
            .unwrap();
        let user = auth
            .create_user(
                &storage,
                "operator",
                "操作员",
                "temporary1",
                UserRole::User,
                110,
            )
            .unwrap();
        auth.logout();
        auth.login(&storage, "operator", "temporary1", 120).unwrap();
        assert_eq!(
            auth.require_user(&storage, 121).unwrap_err(),
            AuthCode::PasswordChangeRequired
        );
        auth.change_own_password(&storage, "temporary1", "replacement1", 122)
            .unwrap();
        assert_eq!(
            auth.require_user(&storage, 123).unwrap().user_id,
            user.user_id
        );
    }
}
