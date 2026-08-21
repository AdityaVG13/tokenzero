
use super::*;

#[test]
fn mask_visible_secrets_covers_cloud_and_token_prefixes() {
    let aws = mask_visible_secrets("AWS_SECRET_ACCESS_KEY=wJalc/not-a-real-secret");
    assert!(aws.contains("AWS_SECRET_ACCESS_KEY=[masked]"), "{aws}");
    assert!(!aws.contains("wJalc"), "{aws}");
    let header = mask_visible_secrets("X-Api-Key: abcd1234secret");
    assert!(
        header
            .to_ascii_lowercase()
            .starts_with("x-api-key:[masked]"),
        "{header}"
    );
    assert!(!header.contains("abcd1234secret"), "{header}");
    let pat = mask_visible_secrets("token github_pat_aaaaaaaa and glpat-bbbbbbbb");
    assert!(!pat.contains("github_pat_aaaaaaaa"), "{pat}");
    assert!(!pat.contains("glpat-bbbbbbbb"), "{pat}");
    assert!(has_visible_secret_marker(
        "AWS_SECRET_ACCESS_KEY=wJalc/not-a-real-secret"
    ));
    assert!(has_visible_secret_marker("github_pat_aaaaaaaa"));
}
