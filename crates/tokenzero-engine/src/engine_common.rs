use super::*;

macro_rules! capsule_response {
    ($tool:expr, $mode:expr, $capsule:expr, $refs:expr, $recovery_tokens:expr) => {{
        let refs = $refs;
        let exact_ref_tokens = exact_ref_token_count(&refs);
        success_response(
            $tool,
            $mode,
            $capsule.text,
            refs,
            (
                $capsule.raw_tokens,
                $capsule.visible_tokens,
                $recovery_tokens,
                Some(exact_ref_tokens),
            ),
        )
    }};
}
pub(super) use capsule_response;

pub(super) fn joined_bytes(parts: &[String]) -> usize {
    parts.iter().map(String::len).sum::<usize>() + parts.len().saturating_sub(1) * 2
}
