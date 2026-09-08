//! Candidate bundles use a separate default profile and cannot update into release builds.
pub(super) fn data_folder() -> &'static str {
    if cfg!(feature = "candidate") {
        "tmh-lpf-900-candidate"
    } else {
        "tmh-lpf-900"
    }
}
pub(super) fn updater_allowed(requested: bool) -> bool {
    requested && !cfg!(feature = "candidate")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidates_keep_their_data_and_update_channel_separate() {
        if cfg!(feature = "candidate") {
            assert_eq!(data_folder(), "tmh-lpf-900-candidate");
            assert!(!updater_allowed(true));
        } else {
            assert_eq!(data_folder(), "tmh-lpf-900");
            assert!(updater_allowed(true));
        }
        assert!(!updater_allowed(false));
    }
}
