use super::*;

pub(crate) fn set_ref_index_test_override(
    value: Option<(bool, PathBuf)>,
) -> Option<(bool, PathBuf)> {
    let new = match value {
        Some((true, path)) => RefIndexOverride::Path(path),
        Some((false, _)) => RefIndexOverride::Disabled,
        None => RefIndexOverride::Isolated,
    };
    match replace_ref_index_override(new) {
        RefIndexOverride::Path(path) => Some((true, path)),
        RefIndexOverride::Disabled => Some((false, PathBuf::new())),
        RefIndexOverride::Ambient | RefIndexOverride::Isolated => None,
    }
}
