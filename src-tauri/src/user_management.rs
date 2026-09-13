//! Audited account mutations behind the native command boundary.
use crate::*;
use serde_json::{json, Value};
use tmh_core::auth::{hash_password, validate_display_name};

fn operator_for(state: &AppState, storage: &Storage) -> String {
    state
        .auth
        .snapshot(storage, epoch_now())
        .ok()
        .and_then(|snapshot| snapshot.user)
        .map_or_else(|| "anonymous".into(), |user| user.display_name)
}

// Outcomes contain only stable error codes. Passwords, hashes and storage diagnostics
// never enter the structured audit payload. Guards are inside the operation so rejected
// attempts are recorded once, including anonymous and locked sessions.
fn finish<T>(
    state: &AppState,
    action: &str,
    operator: String,
    mut detail: Value,
    outcome: Result<T, String>,
) -> Result<T, String> {
    let result = match &outcome {
        Ok(_) => AuditResult::Success,
        Err(code) if code == AuthCode::Storage.as_str() => AuditResult::Failure,
        Err(_) => AuditResult::Rejected,
    };
    detail["code"] = outcome
        .as_ref()
        .err()
        .map_or(Value::Null, |code| json!(code));
    state.audit.write(
        &AuditEntry::new(Category::Auth, action, result)
            .operator(operator)
            .detail(detail),
    );
    outcome
}

pub(crate) fn login(
    state: &AppState,
    username: String,
    password: String,
) -> Result<AuthSnapshot, String> {
    let storage = lock(&state.storage);
    let authenticated = state
        .auth
        .login(&storage, &username, &password, epoch_now());
    // Until credentials are verified, the attempted username is the only attribution.
    // Keep the verified actor even if constructing the final response fails afterward.
    let operator = authenticated
        .as_ref()
        .map_or_else(|_| username.clone(), |user| user.display_name.clone());
    let outcome = authenticated
        .and_then(|_| state.auth.snapshot(&storage, epoch_now()))
        .map_err(auth_error);
    finish(
        state,
        "login",
        operator,
        json!({"username": username}),
        outcome,
    )
}

pub(crate) fn change_own_password(
    state: &AppState,
    current_password: String,
    new_password: String,
) -> Result<(), String> {
    let storage = lock(&state.storage);
    let operator = operator_for(state, &storage);
    let outcome = state
        .auth
        .change_own_password(&storage, &current_password, &new_password, epoch_now())
        .map_err(auth_error);
    finish(state, "change_password", operator, json!({}), outcome)
}

pub(crate) fn bootstrap_admin(
    state: &AppState,
    username: String,
    display_name: String,
    password: String,
) -> Result<PublicUser, String> {
    let storage = lock(&state.storage);
    let prior_operator = operator_for(state, &storage);
    let outcome = state
        .auth
        .bootstrap(&storage, &username, &display_name, &password, epoch_now())
        .map_err(auth_error);
    let operator = outcome
        .as_ref()
        .map_or(prior_operator, |user| user.display_name.clone());
    let detail =
        json!({"username": username, "user_id": outcome.as_ref().ok().map(|user| &user.user_id)});
    finish(
        state,
        "bootstrap_admin",
        operator,
        detail,
        outcome.map(|user| PublicUser::from(&user)),
    )
}

pub(crate) fn create_user(
    state: &AppState,
    username: String,
    display_name: String,
    temporary_password: String,
    role: UserRole,
) -> Result<PublicUser, String> {
    let storage = lock(&state.storage);
    let operator = operator_for(state, &storage);
    let outcome = state
        .auth
        .create_user(
            &storage,
            &username,
            &display_name,
            &temporary_password,
            role,
            epoch_now(),
        )
        .map_err(auth_error);
    let detail = json!({"username": username, "role": role, "user_id": outcome.as_ref().ok().map(|user| &user.user_id)});
    finish(
        state,
        "create_user",
        operator,
        detail,
        outcome.map(|user| PublicUser::from(&user)),
    )
}

pub(crate) fn update_user(
    state: &AppState,
    user_id: String,
    display_name: String,
    role: UserRole,
    is_active: bool,
) -> Result<PublicUser, String> {
    let storage = lock(&state.storage);
    let operator = operator_for(state, &storage);
    let mut detail = json!({"user_id": user_id, "role_to": role, "active_to": is_active});
    let outcome = (|| {
        let actor = state
            .auth
            .require_admin(&storage, epoch_now())
            .map_err(auth_error)?;
        let display_name =
            validate_display_name(&display_name).map_err(|_| AuthCode::InvalidInput.to_string())?;
        let target = storage
            .get_user(&user_id)
            .map_err(|_| AuthCode::Storage.to_string())?
            .ok_or_else(|| "USER_NOT_FOUND".to_string())?;
        if target.user_id == actor.user_id && (role != target.role || !is_active) {
            return Err(AuthCode::Forbidden.to_string());
        }
        if target.role.is_admin()
            && target.is_active
            && (!role.is_admin() || !is_active)
            && storage
                .active_admin_count()
                .map_err(|_| AuthCode::Storage.to_string())?
                <= 1
        {
            return Err(AuthCode::LastAdmin.to_string());
        }
        storage
            .update_user(&user_id, &display_name, role, is_active, &iso_now())
            .map_err(|_| AuthCode::Storage.to_string())?;
        let updated = storage
            .get_user(&user_id)
            .map_err(|_| AuthCode::Storage.to_string())?
            .ok_or_else(|| "USER_NOT_FOUND".to_string())?;
        detail["username"] = json!(updated.username);
        detail["role_from"] = json!(target.role);
        detail["active_from"] = json!(target.is_active);
        Ok(PublicUser::from(&updated))
    })();
    finish(state, "update_user", operator, detail, outcome)
}

pub(crate) fn reset_user_password(
    state: &AppState,
    user_id: String,
    temporary_password: String,
) -> Result<(), String> {
    let storage = lock(&state.storage);
    let operator = operator_for(state, &storage);
    let outcome = (|| {
        let actor = state
            .auth
            .require_admin(&storage, epoch_now())
            .map_err(auth_error)?;
        if actor.user_id == user_id {
            return Err(AuthCode::Forbidden.to_string());
        }
        let hash =
            hash_password(&temporary_password).map_err(|_| AuthCode::InvalidInput.to_string())?;
        if storage
            .get_user(&user_id)
            .map_err(|_| AuthCode::Storage.to_string())?
            .is_none()
        {
            return Err("USER_NOT_FOUND".into());
        }
        storage
            .set_user_password(&user_id, &hash, true, &iso_now())
            .map_err(|_| AuthCode::Storage.to_string())
    })();
    finish(
        state,
        "reset_password",
        operator,
        json!({"user_id": user_id}),
        outcome,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    pub(super) fn fixture() -> AppState {
        let state = crate::tests::test_state();
        std::fs::create_dir_all(state.recovery_dir.parent().unwrap().join("logs")).unwrap();
        state
    }

    pub(super) fn entries(state: &AppState) -> Vec<Value> {
        std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
            .unwrap()
            .flat_map(|file| {
                std::fs::read_to_string(file.unwrap().path())
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect::<Vec<Value>>()
            })
            .collect()
    }

    fn assert_rejected(state: &AppState, before: usize, action: &str, operator: &str, code: &str) {
        let log = entries(state);
        assert_eq!(
            log.len(),
            before + 1,
            "one audit for each attempted mutation"
        );
        let last = log.last().unwrap();
        assert_eq!(last["category"], "AUTH");
        assert_eq!(last["action"], action);
        assert_eq!(last["result"], "REJECTED");
        assert_eq!(last["operator"], operator);
        assert_eq!(last["detail"]["code"], code);
        let text = serde_json::to_string(&log).unwrap();
        assert!(!text.contains("SecretPassword!"));
        assert!(!text.contains("$argon2"));
    }

    pub(super) fn admin(state: &AppState) -> PublicUser {
        bootstrap_admin(
            state,
            "admin".into(),
            "Administrator".into(),
            "SecretPassword!".into(),
        )
        .unwrap()
    }

    #[test]
    fn bootstrap_refusal_and_validation_are_audited_without_passwords() {
        let state = fixture();
        assert!(
            bootstrap_admin(&state, "admin".into(), "".into(), "SecretPassword!".into()).is_err()
        );
        assert_rejected(&state, 0, "bootstrap_admin", "anonymous", "INVALID_PARAMS");
        admin(&state);
        assert!(bootstrap_admin(
            &state,
            "another".into(),
            "Other".into(),
            "SecretPassword!".into()
        )
        .is_err());
        assert_rejected(
            &state,
            2,
            "bootstrap_admin",
            "Administrator",
            "ALREADY_INITIALIZED",
        );
        assert_eq!(lock(&state.storage).user_count().unwrap(), 1);
    }

    #[test]
    fn every_account_mutation_audits_anonymous_regular_and_locked_denials() {
        let state = fixture();
        admin(&state);
        let user = create_user(
            &state,
            "user".into(),
            "Operator".into(),
            "SecretPassword!".into(),
            UserRole::User,
        )
        .unwrap();
        state.auth.logout();
        for (index, (operator, code)) in [
            ("anonymous", "AUTH_REQUIRED"),
            ("Operator", "FORBIDDEN"),
            ("anonymous", "AUTH_REQUIRED"),
        ]
        .into_iter()
        .enumerate()
        {
            if operator == "Operator" {
                state
                    .auth
                    .login(
                        &lock(&state.storage),
                        "user",
                        "SecretPassword!",
                        epoch_now(),
                    )
                    .unwrap();
                state
                    .auth
                    .change_own_password(
                        &lock(&state.storage),
                        "SecretPassword!",
                        "ReplacementPassword!",
                        epoch_now(),
                    )
                    .unwrap();
            } else if index == 2 {
                state.auth.lock();
            }
            let count = entries(&state).len();
            assert_eq!(
                create_user(
                    &state,
                    "blocked".into(),
                    "Blocked".into(),
                    "SecretPassword!".into(),
                    UserRole::Admin
                )
                .unwrap_err(),
                code
            );
            assert_rejected(&state, count, "create_user", operator, code);
            assert_eq!(
                update_user(
                    &state,
                    user.user_id.clone(),
                    "Changed".into(),
                    UserRole::Admin,
                    false
                )
                .unwrap_err(),
                code
            );
            assert_rejected(&state, count + 1, "update_user", operator, code);
            assert_eq!(
                reset_user_password(&state, user.user_id.clone(), "SecretPassword!".into())
                    .unwrap_err(),
                code
            );
            assert_rejected(&state, count + 2, "reset_password", operator, code);
        }
        let unchanged = lock(&state.storage)
            .get_user(&user.user_id)
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.display_name, "Operator");
        assert!(unchanged.is_active);
        assert_eq!(unchanged.role, UserRole::User);
        assert_eq!(lock(&state.storage).user_count().unwrap(), 2);
    }

    #[test]
    fn rejected_admin_inputs_and_self_changes_leave_accounts_unchanged_and_audit_once() {
        let state = fixture();
        let admin = admin(&state);
        for (name, role, active, code) in [
            ("", UserRole::Admin, true, "INVALID_PARAMS"),
            ("Changed", UserRole::User, true, "FORBIDDEN"),
            ("Changed", UserRole::Admin, false, "FORBIDDEN"),
        ] {
            let before = entries(&state).len();
            assert_eq!(
                update_user(&state, admin.user_id.clone(), name.into(), role, active).unwrap_err(),
                code
            );
            assert_rejected(&state, before, "update_user", "Administrator", code);
        }
        let before = entries(&state).len();
        assert_eq!(
            reset_user_password(&state, admin.user_id.clone(), "SecretPassword!".into())
                .unwrap_err(),
            "FORBIDDEN"
        );
        assert_rejected(
            &state,
            before,
            "reset_password",
            "Administrator",
            "FORBIDDEN",
        );
        for password in ["x", "SecretPassword!"] {
            let code = if password == "x" {
                "INVALID_PARAMS"
            } else {
                "USER_NOT_FOUND"
            };
            let before = entries(&state).len();
            assert_eq!(
                reset_user_password(&state, "missing".into(), password.into()).unwrap_err(),
                code
            );
            assert_rejected(&state, before, "reset_password", "Administrator", code);
        }
        assert_eq!(
            lock(&state.storage)
                .get_user(&admin.user_id)
                .unwrap()
                .unwrap()
                .display_name,
            "Administrator"
        );
    }

    #[test]
    fn successful_account_changes_keep_actor_and_before_after_details() {
        let state = fixture();
        admin(&state);
        let user = create_user(
            &state,
            "user".into(),
            "Original".into(),
            "SecretPassword!".into(),
            UserRole::User,
        )
        .unwrap();
        update_user(
            &state,
            user.user_id.clone(),
            "Changed".into(),
            UserRole::Admin,
            true,
        )
        .unwrap();
        reset_user_password(&state, user.user_id.clone(), "ReplacementPassword!".into()).unwrap();
        let log = entries(&state);
        assert_eq!(log.len(), 4);
        assert!(log
            .iter()
            .all(|row| row["result"] == "SUCCESS" && row["operator"] == "Administrator"));
        assert_eq!(log[2]["detail"]["role_from"], "USER");
        assert_eq!(log[2]["detail"]["role_to"], "ADMIN");
        let updated = lock(&state.storage)
            .get_user(&user.user_id)
            .unwrap()
            .unwrap();
        assert!(updated.must_change_password);
        assert_eq!(updated.display_name, "Changed");
        let text = serde_json::to_string(&log).unwrap();
        assert!(!text.contains("Password!"));
        assert!(!text.contains("$argon2"));
    }
}

#[cfg(test)]
#[path = "tests/auth_audit_tests.rs"]
mod auth_audit_tests;
