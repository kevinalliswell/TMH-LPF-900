use super::device_control_authorization_tests::{app_fixture, command_audit, select_actor};
use super::*;

#[test]
fn authenticated_window_close_is_attributed_to_the_current_operator() {
    let (app, _trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    select_actor(&state, "admin");

    assert!(
        !prepare_exit(app.handle()),
        "the idle mock app should close"
    );

    let entries = command_audit(&state, "exit");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["category"], "APP");
    assert_eq!(entries[0]["result"], "SUCCESS");
    assert_eq!(entries[0]["operator"], "管理员");
    assert_eq!(entries[0]["detail"]["source"], "window_close");
    assert!(!prepare_exit_from(app.handle(), ExitOrigin::SystemExit));
    assert_eq!(
        command_audit(&state, "exit").len(),
        1,
        "the subsequent system exit notification must not replace the user attribution"
    );
}

#[test]
fn anonymous_window_close_is_attributed_as_anonymous() {
    let (app, _trace, _closed) = app_fixture();
    let state = app.state::<AppState>();

    assert!(
        !prepare_exit(app.handle()),
        "the idle mock app should close"
    );

    let entries = command_audit(&state, "exit");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["operator"], "anonymous");
    assert_eq!(entries[0]["detail"]["source"], "window_close");
}

#[test]
fn system_exit_keeps_system_attribution_even_with_a_logged_in_user() {
    let (app, _trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    select_actor(&state, "admin");

    assert!(!prepare_exit_from(app.handle(), ExitOrigin::SystemExit));

    let entries = command_audit(&state, "exit");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["operator"], "system");
    assert_eq!(entries[0]["detail"]["source"], "system_exit");
}
