#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Anonymous,
    Active(String),
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    Required,
    Expired,
}

#[derive(Debug, Clone)]
struct ActiveSession {
    user_id: String,
    last_activity_epoch_s: i64,
}

#[derive(Debug)]
pub struct AuthSession {
    active: Option<ActiveSession>,
    locked: bool,
    idle_timeout_s: i64,
}

impl AuthSession {
    pub fn new(idle_timeout_s: i64) -> Self {
        Self {
            active: None,
            locked: false,
            idle_timeout_s,
        }
    }

    pub fn login(&mut self, user_id: String, now_epoch_s: i64) {
        self.active = Some(ActiveSession {
            user_id,
            last_activity_epoch_s: now_epoch_s,
        });
        self.locked = false;
    }

    pub fn logout(&mut self) {
        self.active = None;
        self.locked = false;
    }

    pub fn lock(&mut self) {
        self.active = None;
        self.locked = true;
    }

    pub fn state(&mut self, now_epoch_s: i64) -> SessionState {
        if self.expire_if_idle(now_epoch_s) {
            return SessionState::Locked;
        }
        match &self.active {
            Some(active) => SessionState::Active(active.user_id.clone()),
            None if self.locked => SessionState::Locked,
            None => SessionState::Anonymous,
        }
    }

    pub fn require(&mut self, now_epoch_s: i64) -> Result<String, SessionError> {
        if self.expire_if_idle(now_epoch_s) {
            return Err(SessionError::Expired);
        }
        self.active
            .as_ref()
            .map(|active| active.user_id.clone())
            .ok_or(SessionError::Required)
    }

    pub fn touch(&mut self, now_epoch_s: i64) -> Result<(), SessionError> {
        self.require(now_epoch_s)?;
        if let Some(active) = &mut self.active {
            active.last_activity_epoch_s = now_epoch_s;
        }
        Ok(())
    }

    fn expire_if_idle(&mut self, now_epoch_s: i64) -> bool {
        let expired = self.active.as_ref().is_some_and(|active| {
            now_epoch_s.saturating_sub(active.last_activity_epoch_s) >= self.idle_timeout_s
        });
        if expired {
            self.lock();
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_session_expires_after_fifteen_minutes() {
        let mut session = AuthSession::new(900);
        session.login("u1".into(), 100);
        assert_eq!(session.require(999), Ok("u1".into()));
        assert_eq!(session.require(1000), Err(SessionError::Expired));
        assert_eq!(session.state(1001), SessionState::Locked);
    }

    #[test]
    fn touching_session_extends_deadline() {
        let mut session = AuthSession::new(900);
        session.login("u1".into(), 100);
        session.touch(950).unwrap();
        assert_eq!(session.require(1800), Ok("u1".into()));
        assert_eq!(session.require(1850), Err(SessionError::Expired));
    }

    #[test]
    fn logout_clears_locked_and_active_state() {
        let mut session = AuthSession::new(900);
        session.login("u1".into(), 100);
        session.logout();
        assert_eq!(session.state(200), SessionState::Anonymous);
        assert_eq!(session.require(200), Err(SessionError::Required));
    }
}
