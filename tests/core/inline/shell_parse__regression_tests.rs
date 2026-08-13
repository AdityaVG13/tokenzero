use super::failed_segment;

#[test]
fn later_and_chain_failure_is_attributed_to_its_segment() {
    assert_eq!(
        failed_segment("cd . && false", "", "", Some(1)).as_deref(),
        Some("false")
    );
}
