use super::*;

fn internal_result() -> Value {
    json!({
        "status": "running",
        "pid": 42,
        "exitCode": null,
        "tail": "ok\n",
        "tailUtf8Lossless": true,
        "tailBytes": 3,
        "log": "/private/session/job.log",
        "logBytes": 3,
        "cursor": 3,
        "version": 2,
        "changed": true,
        "unchanged": false,
        "nextPollMs": 20_000,
    })
}

#[test]
fn shared_job_contract_digest_is_canonical_in_the_tokenzero_graph() {
    assert_eq!(
        zero_abi::token_job_contract_digest(),
        "d9b15de5be5a4c5a2d80ffd409eb04fc796b16b377a67254016fc4f285b7a597"
    );
}

#[test]
fn typed_job_result_strips_private_log_and_rejects_unknown_output() {
    let value = typed_job_result("tzjob-7", &internal_result()).unwrap();
    assert!(value.get("log").is_none());
    let typed: TokenJobPollResult = serde_json::from_value(value).unwrap();
    typed.validate().unwrap();

    let mut mutant = internal_result();
    mutant["privateLog"] = json!("/private/session/job.log");
    let error = typed_job_result("tzjob-7", &mutant).unwrap_err();
    assert_eq!(error.kind, "invalid_result");
    assert!(error.message.contains("unknown field"), "{}", error.message);
}

#[test]
fn unchanged_poll_becomes_a_successful_typed_empty_delta() {
    let value = typed_job_result(
        "tzjob-7",
        &json!({
            "status":"running",
            "pid":42,
            "unchanged":true,
            "cursor":9,
            "version":2,
            "nextPollMs":20_000,
        }),
    )
    .unwrap();
    assert_eq!(value["changed"], false);
    assert_eq!(value["tail"], "");
    assert_eq!(value["tailUtf8Lossless"], true);
    assert_eq!(value["tailBytes"], 0);
    assert_eq!(value["logBytes"], 9);
}
