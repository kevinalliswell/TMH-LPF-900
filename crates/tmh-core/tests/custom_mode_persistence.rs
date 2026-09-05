use tmh_core::storage::Storage;
#[test]
fn updating_and_deleting_a_mode_survives_database_reopen() {
    let path = std::env::temp_dir().join(format!("tmh-mode-{}.db", tmh_core::auth::new_user_id()));
    {
        let s = Storage::open(&path).unwrap();
        s.save_custom_program("custom-a", "first").unwrap();
        s.save_custom_program("custom-a", "edited").unwrap();
        s.save_custom_program("custom-b", "second").unwrap();
    }
    let s = Storage::open(&path).unwrap();
    assert_eq!(s.list_custom_programs().unwrap(), vec!["edited", "second"]);
    assert!(s.delete_custom_program("custom-a").unwrap());
    assert_eq!(s.list_custom_programs().unwrap(), vec!["second"]);
    assert!(!s.delete_custom_program("missing").unwrap());
    drop(s);
    std::fs::remove_file(path).unwrap();
}
