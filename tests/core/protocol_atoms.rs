use std::collections::{BTreeMap, BTreeSet};

use ah_ah_ah::{Backend, count_tokens};
use serde::Deserialize;
use tokenzero_core::{
    AckClass, PORTABLE_ONE_TOKEN_ATOMS, ProtocolTokenizer, is_verified_one_token_atom,
    portable_one_token_atoms, render_ack,
};

#[derive(Debug, Deserialize)]
struct Fixture {
    schema: String,
    normalization: String,
    protocol_atoms: Vec<String>,
    portable_intersection: Vec<String>,
    tables: Vec<Table>,
}

#[derive(Debug, Deserialize)]
struct Table {
    id: String,
    tokenizer: String,
    verification: Verification,
    atoms: BTreeMap<String, usize>,
}

#[derive(Debug, Deserialize)]
struct Verification {
    kind: String,
    implementation: String,
    reference: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/one-token-atoms.json"))
    .expect("one-token atom fixture must be valid JSON")
}

#[test]
fn every_protocol_atom_is_one_token_in_every_table() {
    let fixture = fixture();
    assert_eq!(fixture.schema, "tokenzero.one_token_atoms.v1");
    assert_eq!(
        fixture.normalization,
        "UTF-8 bytes, no prefix or suffix, NFC identity"
    );
    assert_eq!(fixture.tables.len(), ProtocolTokenizer::ALL.len());

    let expected_ids = ProtocolTokenizer::ALL
        .into_iter()
        .map(ProtocolTokenizer::as_str)
        .collect::<BTreeSet<_>>();
    let actual_ids = fixture
        .tables
        .iter()
        .map(|table| table.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_ids, expected_ids);

    for table in &fixture.tables {
        assert!(
            !table.tokenizer.is_empty(),
            "{} tokenizer missing",
            table.id
        );
        assert!(
            !table.verification.kind.is_empty(),
            "{} verification kind missing",
            table.id
        );
        assert!(
            !table.verification.implementation.is_empty(),
            "{} verifier missing",
            table.id
        );
        assert!(table.verification.reference.starts_with("https://"));
        for atom in &fixture.protocol_atoms {
            assert_eq!(
                table.atoms.get(atom),
                Some(&1),
                "{atom:?} is not certified as one token for {}",
                table.id
            );
        }
    }
}

#[test]
fn portable_intersection_matches_public_runtime_table() {
    let fixture = fixture();
    let mut intersection = fixture
        .protocol_atoms
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for table in &fixture.tables {
        intersection.retain(|atom| table.atoms.get(atom) == Some(&1));
    }
    let expected = fixture
        .portable_intersection
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(intersection, expected);
    assert_eq!(portable_one_token_atoms(), PORTABLE_ONE_TOKEN_ATOMS);
    assert_eq!(
        expected,
        PORTABLE_ONE_TOKEN_ATOMS
            .iter()
            .map(|atom| (*atom).to_string())
            .collect()
    );
    for tokenizer in ProtocolTokenizer::ALL {
        for atom in PORTABLE_ONE_TOKEN_ATOMS {
            assert!(is_verified_one_token_atom(tokenizer, atom));
        }
        assert!(!is_verified_one_token_atom(tokenizer, "10"));
    }
}

#[test]
fn locally_available_tokenizers_reverify_the_portable_atoms() {
    let o200k = tiktoken_rs::o200k_base().expect("o200k vocabulary must load");
    for atom in PORTABLE_ONE_TOKEN_ATOMS {
        assert_eq!(o200k.encode_ordinary(atom).len(), 1, "o200k: {atom:?}");
        assert_eq!(
            count_tokens(atom, None, Backend::Claude, None).count,
            1,
            "Claude: {atom:?}"
        );
    }
}

#[test]
fn ack2_golden_atoms_are_portable_and_deterministic() {
    let golden: serde_json::Value = serde_json::from_str(include_str!("fixtures/ack2-golden.json"))
    .unwrap();
    let classes = [
        AckClass::Success,
        AckClass::Validation,
        AckClass::Policy,
        AckClass::Substrate,
        AckClass::Retryable,
        AckClass::Internal,
    ];
    for (case, class) in golden["cases"].as_array().unwrap().iter().zip(classes) {
        let atom = case["atom"].as_str().unwrap();
        assert_eq!(render_ack(class, false), atom);
        for tokenizer in ProtocolTokenizer::ALL {
            assert!(is_verified_one_token_atom(tokenizer, atom));
        }
        assert_eq!(render_ack(class, false), render_ack(class, false));
    }
    assert_eq!(render_ack(AckClass::Success, true), "");
    assert_eq!(golden["silent_pure_mutation_success"], "");
}
