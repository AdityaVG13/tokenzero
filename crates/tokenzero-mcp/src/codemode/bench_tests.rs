use super::*;

#[test]
fn recovery_tokens_debit_benchmark_savings_and_may_go_negative() {
    assert_eq!(savings_pct(70, 100), 30.0, "gross visible-only savings");
    assert_eq!(
        savings_pct(70usize.saturating_add(40), 100),
        -10.0,
        "M_rec debit must be allowed to make savings negative"
    );
}

#[test]
fn recovery_tokens_are_read_before_fastmcp_scalar_folding() {
    let response = ToolResponse {
        telemetry: Some(json!({
            "structuredContent": {
                "telemetry": { "recovery_tokens": 40 }
            }
        })),
        ..ToolResponse::default()
    };
    assert_eq!(recovery_tokens_from_response(&response), 40);
}

#[cfg(feature = "surface-mcp")]
struct Fixture {
    root: PathBuf,
    engine: TokenZeroEngine,
}
#[cfg(feature = "surface-mcp")]
impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::current_dir().unwrap();
        let engine = engine_for_leg(&root, hermetic_cache_path(0, tag, "plan"));
        Self { root, engine }
    }
    fn workload(&self, name: &str) -> Workload {
        workloads_for_root(&self.root)
            .into_iter()
            .find(|w| w.name == name)
            .unwrap()
    }
    fn render(&self, plan: &str) -> Vec<String> {
        let response = dispatch_tool(
            &self.engine,
            "execute_code",
            "tz_execute_code",
            &json!({"plan":plan,"envelope":"v2","ref_first":true}),
        )
        .unwrap();
        fastmcp_content_texts_from_tool_result(&mcp_tool_response(response)).unwrap()
    }
    fn measure(&self, plan: &str) -> PlanMeasurement {
        measure_plan_leg(&self.engine, plan)
    }
}

#[cfg(feature = "surface-mcp")]
#[test]
fn scalar_return_plan_folds_into_primary_content() {
    let rendered = Fixture::new("scalar-fold").render("return await Promise.resolve(true)");
    assert_eq!(rendered.len(), 1, "{rendered:?}");
    assert!(rendered[0].contains(" =true t:"), "{rendered:?}");
    assert_eq!(
        rendered[0].matches("=true").count(),
        1,
        "scalar must fold exactly once: {}",
        rendered[0]
    );
    let ack_body = rendered[0]
        .rsplit_once(" t:")
        .map(|(body, _)| body)
        .unwrap_or(&rendered[0]);
    assert!(count_tokens(ack_body) <= 14, "{}", rendered[0]);
}

#[cfg(feature = "surface-mcp")]
#[test]
fn pipe_composition_payload_is_ref_preview() {
    let f = Fixture::new("pipe-payload");
    let measured = f.measure(&f.workload("pipe-composition").plan);
    assert!(measured.payload_tokens < 40, "{measured:?}");
    assert_eq!(measured.wire_texts.len(), 2, "{:?}", measured.wire_texts);
    let value = serde_json::from_str::<Value>(&measured.wire_texts[1])
        .unwrap()
        .get("value")
        .unwrap()
        .clone();
    assert!(value.get("ref").and_then(Value::as_str).is_some());
    assert!(
        value
            .get("preview")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .count()
            <= 32
    );
}

#[cfg(feature = "surface-mcp")]
#[test]
fn codemode_v2_structured_json_is_compact() {
    let measured = Fixture::new("compact-json").measure("return { answer: 42 }");
    assert_eq!(measured.wire_texts.len(), 2);
    let structured = &measured.wire_texts[1];
    for forbidden in [": ", "\n", "null"] {
        assert!(!structured.contains(forbidden), "{structured}");
    }
    let value: Value = serde_json::from_str(structured).unwrap();
    assert!(value.get("refs").is_none());
}

#[cfg(feature = "surface-mcp")]
#[test]
fn matrix_integrity_sums_exact_and_legs_nonzero() {
    let report = run_benchmark(&std::env::current_dir().unwrap());
    assert_eq!(report.version, BENCHMARK_REPORT_VERSION);
    for workload in &report.workloads {
        for (leg, tokens) in [
            ("raw", workload.raw_visible_tokens),
            ("per-op", workload.perop_visible_tokens),
            ("plan", workload.plan_visible_tokens),
            ("payload", workload.payload_tokens),
            ("envelope", workload.envelope_tokens),
        ] {
            assert!(tokens > 0, "{} {leg}", workload.workload);
        }
        assert_eq!(
            workload.payload_tokens + workload.envelope_tokens,
            workload.plan_visible_tokens,
            "{} visible split",
            workload.workload
        );
    }
    macro_rules! total {
        ($total:ident, $field:ident, $label:literal) => {
            assert_eq!(
                report.totals.$total,
                report.workloads.iter().map(|row| row.$field).sum::<usize>(),
                concat!($label, " total")
            );
        };
    }
    total!(total_raw_visible, raw_visible_tokens, "raw");
    total!(total_perop_visible, perop_visible_tokens, "per-op");
    total!(total_perop_args, perop_args_tokens, "per-op args");
    total!(total_plan_visible, plan_visible_tokens, "plan");
    assert_eq!(
        report.totals.total_plan_visible,
        report.totals.total_payload + report.totals.total_envelope
    );
}

#[cfg(feature = "surface-mcp")]
#[test]
fn plan_leg_matches_fastmcp_v2_rendering_byte_for_byte() {
    let f = Fixture::new("render");
    let workload = workloads_for_root(&f.root).remove(0);
    let rendered = f.render(&workload.plan);
    let measured = f.measure(&workload.plan);
    assert_eq!(measured.visible_tokens, wire_tokens(&rendered));
}

#[cfg(feature = "surface-mcp")]
#[test]
fn perop_leg_measures_classic_read_text() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workload = Workload {
        name: "read-only".into(),
        description: "read only".into(),
        scale_workload: false,
        plan: String::new(),
        raw_commands: vec![raw_sh("cat Cargo.toml".into())],
        perop_calls: vec![direct(
            "tz_read",
            "read",
            json!({"path":root.join("Cargo.toml").to_string_lossy().to_string()}),
        )],
    };
    let engine = engine_for_leg(&root, hermetic_cache_path(0, "direct-read", "perop"));
    let measured = measure_perop_leg(&engine, &workload);
    assert!(measured.visible_tokens > 0);
    assert!(
        measured
            .wire_text
            .starts_with("[package]\nname = \"tokenzero-mcp\"")
    );
    assert!(measured.wire_text.contains("tokenzero"));
}

#[cfg(feature = "surface-mcp")]
#[test]
fn codemode_v2_refs_are_capped_to_returned_value_refs() {
    let f = Fixture::new("refs-cap");
    let big = (0..300)
        .map(|index| format!("word{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let rendered = f.render(&format!(
        "return {{ kept: {} }}",
        serde_json::to_string(&big).unwrap()
    ));
    let structured: Value = serde_json::from_str(&rendered[1]).unwrap();
    let value_text = serde_json::to_string(structured.get("value").unwrap()).unwrap();
    let refs = structured.get("refs").and_then(Value::as_array).unwrap();
    assert_eq!(refs.len(), 1);
    let ref_id = refs[0].as_str().unwrap();
    let expanded = f.engine.expand(ref_id, Some("raw"), None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.as_ref().unwrap().text, big);
    for reference in refs {
        assert!(value_text.contains(reference.as_str().unwrap()));
    }
    assert!(!value_text.contains(structured.get("ref").and_then(Value::as_str).unwrap()));
}

#[cfg(feature = "surface-mcp")]
#[test]
fn benchmark_double_run_identity() {
    let root = std::env::current_dir().unwrap();
    let [run1, run2] = [run_benchmark(&root), run_benchmark(&root)];
    assert_eq!(run1.version, run2.version);
    assert_eq!(run1.workloads.len(), run2.workloads.len());
    for (index, (a, b)) in run1.workloads.iter().zip(&run2.workloads).enumerate() {
        assert_eq!(a.workload, b.workload, "workload {index} name mismatch");
        assert_eq!(
            a.plan_text_tokens, b.plan_text_tokens,
            "{} plan_text",
            a.workload
        );
        assert_eq!(a.plan_ops, b.plan_ops, "{} plan_ops", a.workload);
    }
    assert_eq!(run1.totals.total_plan_text, run2.totals.total_plan_text);
}

#[cfg(feature = "surface-mcp")]
#[test]
fn run_composition_benchmark() {
    let report = run_benchmark(&std::env::current_dir().unwrap());
    let document = json!({
        "version": report.version,
        "description": "CodeMode plan composition benchmark: v2 CodeMode FastMCP wire vs raw subprocess output and equivalent classic per-op MCP tool responses",
        "methodology": "All legs run from the TokenZero repo root with the same count_tokens tokenizer. Plan and per-op legs use separate fresh recovery caches per workload. The plan leg calls tz_execute_code with the v2 ref-first CodeMode envelope, counts FastMCP content text exactly as emitted, and debits telemetry.recovery_tokens (M_rec) from recovery-adjusted savings and the headline. The per-op leg calls the classic tz_* tool path for each equivalent operation, counts every FastMCP content text, and reports argument tokens separately. The raw leg executes real subprocess commands with std::process in the repo root and tokenizes the exact command text plus stdout and stderr that an agent without ZeroStack would consume. Raw excludes harness per-call framing, which is conservative in CodeMode's favor.",
        "headline": {"metric":"recovery_adjusted_codemode_vs_raw_savings_pct","value":report.totals.headline_savings_pct},
        "workloads": report.workloads, "totals": report.totals,
    });
    let serialized = serde_json::to_string_pretty(&document).unwrap();
    if let Ok(path) = std::env::var("TOKENZERO_COMPOSITION_BENCHMARK_OUT") {
        std::fs::write(path, serialized).unwrap();
    } else {
        println!("{serialized}");
    }
}
