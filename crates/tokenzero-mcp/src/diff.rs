//! Unified-diff rendering for the session redundancy layer's diff-aware
//! re-reads (docs/routing.md §5b). Hunk headers only — the caller supplies
//! the `# read <path> — changed since served this session` framing and the
//! `full file: expand <ref>` tail, so no `---`/`+++` file header is emitted.

use similar::{ChangeTag, TextDiff};

pub(crate) struct DiffRender {
    pub text: String,
    pub hunks: usize,
    pub plus: usize,
    pub minus: usize,
}

/// Render a unified diff with 3 context lines plus change counts. Returns
/// `None` for identical inputs (no hunks to show).
pub(crate) fn unified_diff(old: &str, new: &str) -> Option<DiffRender> {
    if old == new {
        return None;
    }
    let diff = TextDiff::from_lines(old, new);
    let mut plus = 0usize;
    let mut minus = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => plus += 1,
            ChangeTag::Delete => minus += 1,
            ChangeTag::Equal => {}
        }
    }
    let hunks = diff.unified_diff().context_radius(3).iter_hunks().count();
    let text = diff.unified_diff().context_radius(3).to_string();
    Some(DiffRender {
        text: text.trim_end().to_string(),
        hunks,
        plus,
        minus,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_diff_renders_hunks_without_file_header() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let new = "a\nb\nc\nd\nE\nf\ng\nh\ni\nj\n";
        let render = unified_diff(old, new).unwrap();

        assert_eq!(render.hunks, 1);
        assert_eq!(render.plus, 1);
        assert_eq!(render.minus, 1);
        assert!(render.text.starts_with("@@"), "{}", render.text);
        assert!(render.text.contains("-e"), "{}", render.text);
        assert!(render.text.contains("+E"), "{}", render.text);
        assert!(!render.text.contains("---"), "{}", render.text);
    }

    #[test]
    fn unified_diff_of_identical_text_is_none() {
        assert!(unified_diff("same\n", "same\n").is_none());
    }
}
