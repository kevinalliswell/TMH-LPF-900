use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use std::{fmt, str::FromStr, sync::OnceLock};

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "ADMIN",
            Self::User => "USER",
        }
    }

    pub const fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UserRole {
    type Err = AuthError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ADMIN" => Ok(Self::Admin),
            "USER" => Ok(Self::User),
            _ => Err(AuthError::InvalidRole),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("invalid username")]
    InvalidUsername,
    #[error("invalid display name")]
    InvalidDisplayName,
    #[error("invalid password")]
    InvalidPassword,
    #[error("invalid role")]
    InvalidRole,
    #[error("password hashing failed")]
    PasswordHash,
}

pub fn normalize_username(value: &str) -> Result<String, AuthError> {
    let normalized = value.trim().to_ascii_lowercase();
    if !(3..=32).contains(&normalized.len())
        || !normalized
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(AuthError::InvalidUsername);
    }
    Ok(normalized)
}

pub fn validate_display_name(value: &str) -> Result<String, AuthError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 64 {
        return Err(AuthError::InvalidDisplayName);
    }
    Ok(trimmed.to_string())
}

pub fn validate_password(value: &str) -> Result<(), AuthError> {
    if !(10..=128).contains(&value.chars().count()) {
        return Err(AuthError::InvalidPassword);
    }
    Ok(())
}

pub fn hash_password(value: &str) -> Result<String, AuthError> {
    validate_password(value)?;
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(value.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::PasswordHash)
}

pub fn verify_password(hash: &str, candidate: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(candidate.as_bytes(), &parsed)
        .is_ok()
}

/// Argon2id hash of an unguessable value, used to spend the same time verifying a
/// password for a username that does not exist as for one that does. Without it the
/// "no such user" path returns in microseconds while a real account costs a full
/// Argon2 verification, which leaks account existence to anyone timing the login
/// command. Built once, lazily.
fn decoy_hash() -> &'static str {
    static DECOY: OnceLock<String> = OnceLock::new();
    DECOY.get_or_init(|| {
        // A v4 UUID is 36 chars, so it always satisfies the password policy.
        hash_password(&new_user_id()).unwrap_or_default()
    })
}

/// Verify `candidate` against the decoy hash. ALWAYS false — call it on the
/// "username not found" path so that path costs the same as a real verification.
pub fn verify_absent_user(candidate: &str) -> bool {
    verify_password(decoy_hash(), candidate)
}

/// Build the decoy hash up front. Call once during startup: otherwise the FIRST
/// unknown-username login of the process pays a hash *and* a verification while a
/// real account pays only the verification, and someone who can restart the app
/// between probes could read account existence off that doubled latency.
pub fn warm_password_hashing() {
    let _ = decoy_hash();
}

pub fn new_user_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_normalized_and_validated() {
        assert_eq!(
            normalize_username("  Admin.User-1_2 ").unwrap(),
            "admin.user-1_2"
        );
        assert_eq!(
            normalize_username("ab").unwrap_err(),
            AuthError::InvalidUsername
        );
        assert_eq!(
            normalize_username("张三").unwrap_err(),
            AuthError::InvalidUsername
        );
    }

    #[test]
    fn password_hash_roundtrips_without_containing_plaintext() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(!hash.contains("correct horse battery staple"));
        assert!(verify_password(&hash, "correct horse battery staple"));
        assert!(!verify_password(&hash, "wrong password"));
    }

    #[test]
    fn password_policy_rejects_short_and_overlong_values() {
        assert_eq!(
            validate_password("short").unwrap_err(),
            AuthError::InvalidPassword
        );
        assert_eq!(
            validate_password(&"x".repeat(129)).unwrap_err(),
            AuthError::InvalidPassword
        );
        validate_password("1234567890").unwrap();
    }

    #[test]
    fn warming_makes_the_decoy_ready_before_the_first_login() {
        warm_password_hashing();
        assert!(decoy_hash().starts_with("$argon2id$"));
        // Idempotent: the second call reuses the same hash rather than rebuilding it.
        let first = decoy_hash();
        warm_password_hashing();
        assert_eq!(first, decoy_hash());
    }

    #[test]
    fn absent_user_verification_always_fails_but_does_the_work() {
        // The decoy path must never authenticate anything, and must actually run the
        // hash so the timing matches a real account (#46).
        assert!(!verify_absent_user("any password at all"));
        assert!(!verify_absent_user(""));
        assert!(decoy_hash().starts_with("$argon2id$"));
    }

    #[test]
    fn roles_use_stable_database_values() {
        assert_eq!(UserRole::Admin.as_str(), "ADMIN");
        assert_eq!(UserRole::User.as_str(), "USER");
        assert_eq!("ADMIN".parse::<UserRole>().unwrap(), UserRole::Admin);
        assert!("OWNER".parse::<UserRole>().is_err());
    }
}
