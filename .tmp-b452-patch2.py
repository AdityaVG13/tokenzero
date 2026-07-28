"""b452 phase 2: canonical authorization at begin_js_host_op."""

P = "crates/tokenzero-mcp/src/codemode/exec.rs"
s = open(P).read()

HELPER = '''
/// Canonical mutation authorization for the quickjs dispatch boundary
/// (tokenzero-b452): deny the workspace-mutating edit family by resolved
/// effect metadata (cluster == "edit"), covering canonical, alias, and
/// legacy spellings alike. Unknown names are not denied here; the dispatcher
/// fails closed on them with an unknown-method error.
fn quickjs_edit_dispatch_denied(method: &str) -> bool {
    if is_journaled_edit(method) {
        return true;
    }
    matches!(
        tokenzero_core::operation_abi::resolve_operation(method),
        Some(op)
            if op.cluster == "edit"
                && op.mutability == tokenzero_core::operation_abi::Mutability::WorkspaceMutating
    )
}
'''

anchor_fn = '''fn is_journaled_edit(method: &str) -> bool {
    matches!(method, "zero.edit" | "edit" | "zero.token.edit" | "tz_edit")
}
'''
assert s.count(anchor_fn) == 1
s = s.replace(anchor_fn, anchor_fn + HELPER)

GATE = '''    // Canonical dispatch authorization (tokenzero-b452): mutation denial is
    // decided from the resolved operation's effect metadata at this dispatch
    // boundary, not from scanning plan source. The quickjs bridge has no
    // journal/transaction support, so the workspace-mutating edit family is
    // refused regardless of alias, computed, or obfuscated spellings.
    if quickjs_edit_dispatch_denied(method) {
        let message = crate::annotate_write_failure(
            concat!(
                "sandbox: mutating binding denied without transaction support ",
                "(use the lowered zero.edit / tz_edit path, not free-form JS mutation)",
            ),
            false,
        );
        let id = async_rt.next_id.fetch_add(1, Ordering::Relaxed);
        let job = Arc::new(AsyncHostJob {
            result: Mutex::new(Some(tz_error_json(&message, "mutating binding denied"))),
            method: method.to_string(),
            tracks_wave: false,
            applied: Mutex::new(false),
        });
        async_rt
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, job);
        return id.to_string();
    }
'''

anchor_gate = "    let logical_width = logical_width_for_method(method, &args);\n"
assert s.count(anchor_gate) == 1
s = s.replace(anchor_gate, GATE + anchor_gate)

open(P, "w").write(s)
print("phase2 ok")
