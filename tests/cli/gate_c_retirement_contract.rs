//! Gate-C proof that TokenZero retains domain truth without an engine-local planner.

use tokenzero_core::operation_abi::{Mutability, operation_by_name};
use tokenzero_core::{ProtocolTokenizer, is_verified_one_token_atom, portable_one_token_atoms};

#[test]
fn aggregate_bindings_effects_and_protocol_atoms_remain_owned() {
    for (tool, binding) in [
        ("tz_read", "zero.read"),
        ("tz_find", "zero.find"),
        ("tz_tree", "zero.tree"),
        ("tz_shell", "zero.shell"),
        ("tz_edit", "zero.edit"),
        ("tz_expand", "zero.token.expand"),
    ] {
        let operation = operation_by_name(tool).unwrap_or_else(|| panic!("missing {tool}"));
        assert_eq!(operation.exposure.codemode_binding, Some(binding));
    }
    assert_eq!(
        operation_by_name("tz_read").expect("read").mutability,
        Mutability::ReadOnly
    );
    assert_eq!(
        operation_by_name("tz_edit").expect("edit").mutability,
        Mutability::WorkspaceMutating
    );

    for atom in portable_one_token_atoms() {
        for tokenizer in ProtocolTokenizer::ALL {
            assert!(
                is_verified_one_token_atom(tokenizer, atom),
                "{atom:?} lost tokenizer verification for {tokenizer:?}"
            );
        }
    }
}
