use base64::{engine::general_purpose, Engine as _};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use std::path::Path;

const ALGORITHM: &str = "PBKDF2-HMAC-SHA256";
const ITERATIONS: u32 = 600_000;
const DEFAULT_ADMIN_PASSWORD: &str = "1952";
const HASH_LEN: usize = 32;
const SALT_LEN: usize = 16;

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminAuthStatus {
    pub initialized: bool,
    pub must_change: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AuthConfig {
    algorithm: String,
    iterations: u32,
    salt_b64: String,
    hash_b64: String,
    must_change: bool,
}

pub fn status(path: &Path) -> Result<AdminAuthStatus, String> {
    let cfg = ensure_config(path)?;
    Ok(AdminAuthStatus {
        initialized: true,
        must_change: cfg.must_change,
    })
}

pub fn verify(path: &Path, password: &str) -> Result<(), String> {
    let cfg = ensure_config(path)?;
    if verify_config(&cfg, password)? {
        Ok(())
    } else {
        Err("AUTH_FAILED".into())
    }
}

pub fn change_password(path: &Path, old_password: &str, new_password: &str) -> Result<(), String> {
    let new_password = new_password.trim();
    if new_password.len() < 4 {
        return Err("新口令至少需要 4 位".into());
    }
    verify(path, old_password)?;
    save_config(path, &new_config(new_password, false)?)
}

fn ensure_config(path: &Path) -> Result<AuthConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let cfg: AuthConfig =
                serde_json::from_str(&text).map_err(|e| format!("管理员口令文件解析失败: {e}"))?;
            validate_config(&cfg)?;
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let cfg = new_config(DEFAULT_ADMIN_PASSWORD, true)?;
            save_config(path, &cfg)?;
            Ok(cfg)
        }
        Err(e) => Err(format!("读取管理员口令失败: {e}")),
    }
}

fn validate_config(cfg: &AuthConfig) -> Result<(), String> {
    if cfg.algorithm != ALGORITHM {
        return Err("不支持的管理员口令算法".into());
    }
    if cfg.iterations != ITERATIONS {
        return Err("不支持的管理员口令迭代次数".into());
    }
    general_purpose::STANDARD
        .decode(&cfg.salt_b64)
        .map_err(|_| "管理员口令盐值无效".to_string())?;
    general_purpose::STANDARD
        .decode(&cfg.hash_b64)
        .map_err(|_| "管理员口令哈希无效".to_string())?;
    Ok(())
}

fn new_config(password: &str, must_change: bool) -> Result<AuthConfig, String> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::getrandom(&mut salt).map_err(|e| format!("生成口令盐值失败: {e}"))?;
    let hash = hash_password(password, &salt);
    Ok(AuthConfig {
        algorithm: ALGORITHM.into(),
        iterations: ITERATIONS,
        salt_b64: general_purpose::STANDARD.encode(salt),
        hash_b64: general_purpose::STANDARD.encode(hash),
        must_change,
    })
}

fn save_config(path: &Path, cfg: &AuthConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建口令配置目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("写入口令配置失败: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("保存口令配置失败: {e}"))?;
    Ok(())
}

fn verify_config(cfg: &AuthConfig, password: &str) -> Result<bool, String> {
    validate_config(cfg)?;
    let salt = general_purpose::STANDARD
        .decode(&cfg.salt_b64)
        .map_err(|_| "管理员口令盐值无效".to_string())?;
    let expected = general_purpose::STANDARD
        .decode(&cfg.hash_b64)
        .map_err(|_| "管理员口令哈希无效".to_string())?;
    Ok(constant_time_eq(&hash_password(password, &salt), &expected))
}

fn hash_password(password: &str, salt: &[u8]) -> [u8; HASH_LEN] {
    let mut hash = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, ITERATIONS, &mut hash);
    hash
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tmh_auth_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("auth.json")
    }

    #[test]
    fn default_password_is_created_and_marked_for_change() {
        let path = auth_path("default");
        let s = status(&path).unwrap();
        assert!(s.initialized);
        assert!(s.must_change);
        assert!(verify(&path, DEFAULT_ADMIN_PASSWORD).is_ok());
        assert!(verify(&path, "wrong").is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn password_change_clears_must_change_flag() {
        let path = auth_path("change");
        change_password(&path, DEFAULT_ADMIN_PASSWORD, "2468").unwrap();
        assert!(verify(&path, DEFAULT_ADMIN_PASSWORD).is_err());
        assert!(verify(&path, "2468").is_ok());
        assert!(!status(&path).unwrap().must_change);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
