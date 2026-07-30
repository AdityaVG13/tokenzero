//! Pinned regression for the tokenzero-54br counterexample class: tiny inputs
//! under tiny budgets must not inflate through the lossy declaration path.
use tokenzero_core::{Mode, count_tokens, make_capsule};

#[test]
fn tiny_inputs_never_inflate() {
    let texts = vec![
        "a b c d e f g h i j k l m n o".to_string(),
        (1..=15)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        (1..=8)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "x".repeat(15),
        "The quick brown fox jumps over the lazy dog and runs fast away".to_string(),
    ];
    for t in &texts {
        let raw = count_tokens(t);
        for budget in [1usize, 22, 441] {
            for label in [Some("shell"), Some("probe"), None] {
                let c = make_capsule(t, Mode::Auto, budget, label);
                assert!(
                    c.visible_tokens <= raw,
                    "INFLATE raw={raw} budget={budget} visible={} label={label:?} text={t:?}",
                    c.visible_tokens
                );
            }
        }
    }
}
