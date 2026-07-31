use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokenzero_core::{ContentType, Mode, ToolResponse, count_tokens, make_capsule};
use tokenzero_engine::config::EngineConfig;
use tokenzero_engine::expand_params::ExpandParams;
use tokenzero_engine::{DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES, TokenZeroEngine};
use tokenzero_recovery::{RecoveryConfig, RecoveryStore, set_ref_index_root_override};

const MANIFEST: &str = include_str!("../../benchmarks/pilot/manifest-v1.json");
const FROZEN_MANIFEST_SHA256: &str =
    "1a27b7dd92f37fca8696ef85ec59f1d12ca0edccac1f69c3ec0a41dc6d25bc1b";
const MASKED_SECRET: &str = "pilot-secret-do-not-store";

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: String,
    suite_version: String,
    frozen: bool,
    seed: u64,
    network: String,
    tasks: Vec<Task>,
}

#[derive(Debug, Deserialize)]
struct Task {
    id: u8,
    name: String,
    fixtures: Vec<Fixture>,
    success_predicate: String,
    anchors: Vec<String>,
    demand_count: usize,
    expected_expand_count: usize,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct TaskCostSummary {
    record_type: &'static str,
    task_id: String,
    task_name: String,
    mode: &'static str,
    engine: &'static str,
    seed: u64,
    success: bool,
    visible: u64,
    expand: u64,
    retries: u64,
    fails: u64,
    ratc: f64,
    expand_count: u64,
    dangling_refs: u64,
    anchor_recall: u8,
    read_count: u64,
}

#[derive(Debug, Serialize, PartialEq)]
struct Report {
    schema_version: &'static str,
    suite_version: String,
    seed: u64,
    network: String,
    results: Vec<TaskCostSummary>,
}

#[derive(Debug, Serialize, PartialEq)]
struct ModeAggregate {
    mode: &'static str,
    task_count: usize,
    success_rate: f64,
    anchor_recall: f64,
    ratc_mean: f64,
    ratc_median: f64,
    visible_tokens: u64,
    expand_tokens: u64,
    expand_rate: f64,
    expand_rate_denominator: &'static str,
    dangling_ref_rate: f64,
    retries: u64,
    fails: u64,
}

#[derive(Debug, Serialize, PartialEq)]
struct TaskDelta {
    success: i8,
    anchor_recall: i16,
    visible: i128,
    expand: i128,
    retries: i128,
    fails: i128,
    ratc: f64,
    expand_count: i128,
    dangling_refs: i128,
}

#[derive(Debug, Serialize, PartialEq)]
struct TaskComparison {
    task_id: String,
    task_name: String,
    baseline: TaskCostSummary,
    exact_ref: TaskCostSummary,
    delta_exact_ref_minus_baseline: TaskDelta,
}

#[derive(Debug, Serialize, PartialEq)]
struct ParetoPair {
    task_id: String,
    task_name: String,
    baseline_visible: u64,
    baseline_ratc: f64,
    exact_ref_visible: u64,
    exact_ref_ratc: f64,
}

#[derive(Debug, Serialize, PartialEq)]
struct AbReport {
    schema_version: &'static str,
    source_schema_version: &'static str,
    suite_version: String,
    seed: u64,
    network: String,
    limitation: &'static str,
    aggregates: Vec<ModeAggregate>,
    per_task: Vec<TaskComparison>,
    pareto_pairs: Vec<ParetoPair>,
}

#[derive(Debug)]
struct EvictionOutcome {
    recovered: String,
    expand_tokens: u64,
    expand_count: u64,
    fails: u64,
    dangling_refs: u64,
    successful_expands: usize,
    expected_misses: usize,
}

struct RefIndexOverrideGuard;

impl Drop for RefIndexOverrideGuard {
    fn drop(&mut self) {
        set_ref_index_root_override(None);
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/pilot")
}

fn load_manifest() -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_str(MANIFEST)?;
    if manifest.schema_version != "tokenzero.pilot-suite.v1"
        || manifest.suite_version != "e4.1-v1"
        || !manifest.frozen
        || manifest.network != "disabled"
        || manifest.tasks.len() != 14
    {
        bail!("invalid frozen pilot manifest metadata")
    }
    for (index, task) in manifest.tasks.iter().enumerate() {
        if task.id as usize != index + 1
            || task.name.is_empty()
            || task.fixtures.is_empty()
            || task.success_predicate.is_empty()
            || task.anchors.is_empty()
            || task.demand_count != task.expected_expand_count
        {
            bail!("incomplete task {} metadata", task.id)
        }
    }
    Ok(manifest)
}

fn load_task(task: &Task) -> Result<(Vec<PathBuf>, String)> {
    let mut paths = Vec::new();
    let mut payload = String::new();
    for fixture in &task.fixtures {
        let path = fixture_root().join(&fixture.path);
        let bytes = fs::read(&path).with_context(|| fixture.path.clone())?;
        if sha256_bytes(&bytes) != fixture.sha256 {
            bail!("fixture digest mismatch: {}", fixture.path)
        }
        payload.push_str(std::str::from_utf8(&bytes)?);
        paths.push(path);
    }
    if task.name == "secret-mask" {
        let raw = payload.replace("[MASKED]", MASKED_SECRET);
        if !raw.contains(MASKED_SECRET) {
            bail!("secret-mask fixture did not exercise a pre-mask secret")
        }
        payload = raw.replace(MASKED_SECRET, "[MASKED]");
        if payload.contains(MASKED_SECRET) {
            bail!("mask-before-store failed")
        }
    }
    Ok((paths, payload))
}

fn expected_reads(name: &str, fixture_count: usize) -> usize {
    match name {
        "multi-file-rename" => 3,
        "config-drift" => 8,
        "eviction-stress" => 6,
        "read-heavy" => 10,
        _ => fixture_count,
    }
}

fn read_window(name: &str, read_index: usize) -> (Option<usize>, Option<usize>) {
    let line = match name {
        "config-drift" if read_index < 8 => Some(read_index + 1),
        "eviction-stress" if read_index < 6 => Some(read_index + 1),
        "read-heavy" if read_index < 10 => Some(read_index + 1),
        _ => None,
    };
    (line, line)
}

fn expand_target(
    task: &Task,
    demand: usize,
    ref_count: usize,
) -> (usize, Option<usize>, Option<usize>) {
    match task.name.as_str() {
        "multi-file-rename" => (demand, None, None),
        "error-blocks" => (0, Some([3, 6][demand]), Some([3, 6][demand])),
        "config-drift" => ([3, 5][demand], None, None),
        "test-triage" => (0, Some(4), Some(4)),
        "sequential-demands" => (0, Some(demand + 1), Some(demand + 1)),
        "read-heavy" => ([3, 7][demand], None, None),
        _ => (demand % ref_count, None, None),
    }
}

fn engine_config(root: &Path, seed: u64, task: u8, mode: &str) -> EngineConfig {
    let mut config = EngineConfig::for_root(root);
    config.cache_path =
        std::env::temp_dir().join(format!("tokenzero-pilot-engine-{seed}-{task}-{mode}.json"));
    config.capsule_exact_ref_threshold_bytes = DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES;
    config.session_dedup = false;
    config
}

fn response_text(response: &ToolResponse) -> String {
    response
        .visible
        .as_ref()
        .map(|visible| visible.text.clone())
        .unwrap_or_default()
}

fn response_tokens(response: &ToolResponse) -> u64 {
    response
        .accounting
        .as_ref()
        .map(|accounting| accounting.visible_tokens as u64)
        .unwrap_or_else(|| count_tokens(&response_text(response)) as u64)
}

fn response_reason(response: &ToolResponse) -> String {
    response
        .error
        .as_ref()
        .map(|error| error.code.clone())
        .or_else(|| {
            response
                .diagnostic
                .as_ref()
                .map(|diagnostic| diagnostic.code.clone())
        })
        .unwrap_or_else(|| response.status.clone())
}

fn response_blob_ref(response: &ToolResponse) -> Result<String> {
    response
        .refs
        .iter()
        .find(|record| record.live && record.ref_id.starts_with("tz://blob/"))
        .map(|record| record.ref_id.clone())
        .context("engine read emitted no live canonical blob ref")
}

fn mode_specific_baseline_anchor(task: &Task, anchor: &str) -> bool {
    matches!(
        (task.name.as_str(), anchor),
        ("eviction-stress", "dangling-ref")
            | ("declared-lossy", "LOSSY_WARNING")
            | (
                "declared-lossy",
                "lossy_policy_id=tokenzero.visible-compression.v1"
            )
            | ("nonexistent-ref", "ref-not-found")
    )
}

fn anchors_recalled(task: &Task, mode: &str, observed: &str) -> bool {
    task.anchors.iter().all(|anchor| {
        (mode == "baseline" && mode_specific_baseline_anchor(task, anchor))
            || observed.contains(anchor)
    })
}

fn predicate_holds(
    task: &Task,
    mode: &str,
    observed: &str,
    recovered: &str,
    payload: &str,
    mask_ok: bool,
    fails: u64,
    dangling_refs: u64,
) -> bool {
    if task.success_predicate.is_empty() || !anchors_recalled(task, mode, observed) {
        return false;
    }
    if mode == "baseline" {
        return true;
    }
    match task.name.as_str() {
        "multi-file-rename" => task.anchors.iter().all(|anchor| recovered.contains(anchor)),
        "error-blocks" => {
            recovered.contains("ERROR E_PARSE")
                && recovered.contains("ERROR E_RANGE")
                && !recovered.contains("PILOT_POLICY_PADDING")
        }
        "config-drift" => {
            recovered.contains("crate_b.edition") && recovered.contains("crate_d.edition")
        }
        "sequential-demands" => task.anchors.iter().all(|anchor| recovered.contains(anchor)),
        "eviction-stress" => {
            recovered.contains("EVICT_0")
                && recovered.contains("EVICT_5")
                && recovered.contains("dangling-ref")
                && fails == 1
                && dangling_refs == 1
        }
        "secret-mask" => {
            mask_ok
                && recovered.trim_end() == payload.trim_end()
                && !recovered.contains(MASKED_SECRET)
        }
        "read-heavy" => {
            recovered.contains("file04=NEEDED_ALPHA")
                && recovered.contains("file08=NEEDED_BETA")
                && !recovered.contains("PILOT_POLICY_PADDING")
        }
        "cross-session-reuse" => recovered.contains("CROSS_SESSION_SENTINEL"),
        "declared-lossy" => {
            observed.contains("mode=lossy")
                && observed.contains("lossy_policy_id=tokenzero.visible-compression.v1")
        }
        "nonexistent-ref" => {
            fails == 1
                && observed.contains("ref-not-found")
                && observed.contains("FALLBACK_SENTINEL")
        }
        _ => task.anchors.iter().all(|anchor| recovered.contains(anchor)),
    }
}

fn run_eviction_stress(payload: &str) -> Result<EvictionOutcome> {
    let isolated_ref_index = std::env::temp_dir().join(format!(
        "tokenzero-pilot-ref-index-{}-{}",
        std::process::id(),
        &sha256_bytes(payload.as_bytes())[..16],
    ));
    set_ref_index_root_override(Some(isolated_ref_index));
    let _ref_index_guard = RefIndexOverrideGuard;
    let payloads = payload.lines().take(6).collect::<Vec<_>>();
    if payloads.len() != 6 || !payloads[0].contains("EVICT_0") || !payloads[5].contains("EVICT_5") {
        bail!("eviction fixture must begin with six ordered payloads")
    }
    let config = RecoveryConfig {
        max_blobs: 5,
        max_bytes: usize::MAX,
        ..RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);
    let range = store.reserve_ordinal_range(payloads.len() as u64)?;
    let mut aliases = Vec::with_capacity(payloads.len());
    for (index, text) in payloads.iter().enumerate() {
        let blob_ref = store.store_blob(text, ContentType::Unknown)?;
        let alias = store.store_ordinal_alias_deferred(range, index as u64, &blob_ref)?;
        store.persist_pending()?;
        aliases.push(alias);
    }

    let miss = store.expand(&aliases[0], None, None, None, None, None);
    if miss.found || miss.reason != "dangling-ref" {
        bail!(
            "evicted ordinal ref did not produce dangling-ref: {}",
            miss.reason
        )
    }
    let hit = store.expand(&aliases[5], None, None, None, None, None);
    if !hit.found || hit.content != payloads[5] {
        bail!("live eviction ref failed exact-byte recovery")
    }
    let fallback = payloads[0];
    Ok(EvictionOutcome {
        recovered: format!("{fallback}\n{}\n{}", miss.reason, hit.content),
        expand_tokens: (count_tokens(fallback) + count_tokens(&hit.content)) as u64,
        expand_count: 2,
        fails: 1,
        dangling_refs: 1,
        successful_expands: 1,
        expected_misses: 1,
    })
}

fn run_task(manifest: &Manifest, task: &Task, mode: &'static str) -> Result<TaskCostSummary> {
    let (paths, payload) = load_task(task)?;
    let reads = expected_reads(&task.name, paths.len());
    let config = engine_config(&fixture_root(), manifest.seed, task.id, mode);
    let engine = TokenZeroEngine::new(config.clone());
    let mut observed = String::new();
    let mut refs = Vec::new();
    let mut ref_texts = Vec::new();
    let mut visible_tokens = 0_u64;

    for read_index in 0..reads {
        let path = &paths[read_index % paths.len()];
        let (start_line, end_line) = read_window(&task.name, read_index);
        let response = if mode == "baseline" {
            engine.read(
                std::slice::from_ref(path),
                Mode::Passthrough,
                start_line,
                end_line,
                true,
                1,
                1_000_000,
            )
        } else {
            engine.read(
                std::slice::from_ref(path),
                Mode::Auto,
                start_line,
                end_line,
                false,
                1,
                4_000,
            )
        };
        if response.status != "ok" {
            bail!(
                "task {} engine read failed: {}",
                task.id,
                response_reason(&response)
            )
        }
        let text = response_text(&response);
        observed.push_str(&text);
        observed.push('\n');
        visible_tokens = visible_tokens.saturating_add(response_tokens(&response));
        if mode == "exact-ref" && task.expected_expand_count > 0 {
            refs.push(response_blob_ref(&response)?);
            ref_texts.push(text);
        }
    }

    if mode == "exact-ref"
        && task.name != "declared-lossy"
        && task.name != "eviction-stress"
        && task.expected_expand_count > 0
        && refs.is_empty()
    {
        bail!("task {} emitted no exact recovery ref", task.id)
    }

    let mut recovered = String::new();
    let mut expand_tokens = 0_u64;
    let mut expand_count = 0_u64;
    let mut fails = 0_u64;
    let mut dangling_refs = 0_u64;
    let mut expected_misses = 0_usize;
    let mut successful_expands = 0_usize;

    if mode == "exact-ref" && task.name == "declared-lossy" {
        let capsule = make_capsule(
            &payload,
            Mode::Auto,
            60,
            Some("benchmarks/pilot/fixtures/task13.txt"),
        );
        capsule
            .validate_omission_rule(&payload)
            .map_err(anyhow::Error::msg)?;
        if capsule.lossy_policy_id.as_deref() != Some("tokenzero.visible-compression.v1") {
            bail!("task 13 missing real lossy policy")
        }
        observed = capsule.text;
        visible_tokens = capsule.visible_tokens as u64;
    } else if mode == "exact-ref" && task.name == "eviction-stress" {
        let outcome = run_eviction_stress(&payload)?;
        recovered = outcome.recovered;
        observed.push_str(&recovered);
        expand_tokens = outcome.expand_tokens;
        expand_count = outcome.expand_count;
        fails = outcome.fails;
        dangling_refs = outcome.dangling_refs;
        successful_expands = outcome.successful_expands;
        expected_misses = outcome.expected_misses;
    } else if mode == "exact-ref" {
        let reopened;
        let expand_engine = if task.name == "cross-session-reuse" {
            reopened = TokenZeroEngine::new(config.clone());
            &reopened
        } else {
            &engine
        };
        for demand in 0..task.expected_expand_count {
            let (ref_index, start_line, end_line) = if task.name == "config-drift" {
                let anchor = ["crate_b.edition", "crate_d.edition"][demand];
                let index = ref_texts
                    .iter()
                    .position(|text| text.contains(anchor))
                    .with_context(|| {
                        format!("task {} missing config-drift anchor {anchor}", task.id)
                    })?;
                (index, None, None)
            } else {
                expand_target(task, demand, refs.len())
            };
            let requested = if task.name == "nonexistent-ref" {
                "tz://blob/ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    .to_string()
            } else {
                refs[ref_index].clone()
            };
            let response = expand_engine.expand_with_params(ExpandParams {
                ref_id: requested,
                selector: Some("raw".to_string()),
                start_line,
                end_line,
                raw: true,
                ..ExpandParams::default()
            });
            expand_count = expand_count.saturating_add(1);
            if response.status == "ok" {
                let text = response_text(&response);
                if !payload.contains(&text) {
                    bail!("task {} recovered non-fixture bytes", task.id)
                }
                observed.push_str(&text);
                observed.push('\n');
                recovered.push_str(&text);
                recovered.push('\n');
                expand_tokens = expand_tokens.saturating_add(
                    response
                        .accounting
                        .as_ref()
                        .map(|accounting| {
                            accounting.recovery_tokens.max(accounting.visible_tokens) as u64
                        })
                        .unwrap_or_else(|| count_tokens(&text) as u64),
                );
                successful_expands += 1;
            } else {
                fails = fails.saturating_add(1);
                let reason = response_reason(&response);
                if task.name == "nonexistent-ref" && reason == "ref_not_found" {
                    expected_misses += 1;
                    observed.push_str("ref-not-found\n");
                    observed.push_str(&payload);
                    recovered.push_str(&payload);
                    expand_tokens = expand_tokens.saturating_add(count_tokens(&payload) as u64);
                } else {
                    bail!("task {} unexpected typed miss: {}", task.id, reason)
                }
            }
        }
    }

    let expected_successes = task.expected_expand_count.saturating_sub(expected_misses);
    let cache_bytes = fs::read(&config.cache_path).unwrap_or_default();
    let mask_ok = task.name != "secret-mask"
        || (!payload.contains(MASKED_SECRET)
            && !cache_bytes
                .windows(MASKED_SECRET.len())
                .any(|window| window == MASKED_SECRET.as_bytes()));
    let predicate_ok = predicate_holds(
        task,
        mode,
        &observed,
        &recovered,
        &payload,
        mask_ok,
        fails,
        dangling_refs,
    );
    let success = (mode == "baseline" || successful_expands == expected_successes)
        && fails as usize == expected_misses
        && predicate_ok;
    let ratc = (visible_tokens + expand_tokens + fails.saturating_mul(1_000)) as f64;

    Ok(TaskCostSummary {
        record_type: "TaskCostSummary",
        task_id: task.id.to_string(),
        task_name: task.name.clone(),
        mode,
        engine: "tokenzero-engine::TokenZeroEngine",
        seed: manifest.seed,
        success,
        visible: visible_tokens,
        expand: expand_tokens,
        retries: 0,
        fails,
        ratc,
        expand_count,
        dangling_refs,
        anchor_recall: u8::from(anchors_recalled(task, mode, &observed)),
        read_count: reads as u64,
    })
}

fn run() -> Result<Report> {
    let manifest = load_manifest()?;
    let mut results = Vec::with_capacity(28);
    for task in &manifest.tasks {
        results.push(run_task(&manifest, task, "baseline")?);
        results.push(run_task(&manifest, task, "exact-ref")?);
    }
    Ok(Report {
        schema_version: "tokenzero.task-cost-summary.v2",
        suite_version: manifest.suite_version,
        seed: manifest.seed,
        network: manifest.network,
        results,
    })
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn aggregate(mode: &'static str, rows: &[&TaskCostSummary]) -> Result<ModeAggregate> {
    if rows.is_empty() {
        bail!("cannot aggregate empty {mode} rows")
    }
    let mut ratc: Vec<f64> = rows.iter().map(|row| row.ratc).collect();
    ratc.sort_by(f64::total_cmp);
    let middle = ratc.len() / 2;
    let ratc_median = if ratc.len() % 2 == 0 {
        (ratc[middle - 1] + ratc[middle]) / 2.0
    } else {
        ratc[middle]
    };
    let visible_tokens = rows.iter().map(|row| row.visible).sum();
    let expand_tokens = rows.iter().map(|row| row.expand).sum();
    let expand_count = rows.iter().map(|row| row.expand_count).sum();
    let read_count = rows.iter().map(|row| row.read_count).sum();
    let dangling_refs = rows.iter().map(|row| row.dangling_refs).sum();
    Ok(ModeAggregate {
        mode,
        task_count: rows.len(),
        success_rate: rows.iter().filter(|row| row.success).count() as f64 / rows.len() as f64,
        anchor_recall: rows.iter().map(|row| row.anchor_recall as u64).sum::<u64>() as f64
            / rows.len() as f64,
        ratc_mean: ratc.iter().sum::<f64>() / ratc.len() as f64,
        ratc_median,
        visible_tokens,
        expand_tokens,
        expand_rate: ratio(expand_count, read_count),
        expand_rate_denominator: "read_count",
        dangling_ref_rate: ratio(dangling_refs, expand_count),
        retries: rows.iter().map(|row| row.retries).sum(),
        fails: rows.iter().map(|row| row.fails).sum(),
    })
}

fn build_ab_report(source: &Report) -> Result<AbReport> {
    if source.results.len() % 2 != 0 {
        bail!("TaskCostSummary stream has an unpaired row")
    }
    let mut per_task = Vec::with_capacity(source.results.len() / 2);
    let mut pareto_pairs = Vec::with_capacity(source.results.len() / 2);
    for pair in source.results.chunks_exact(2) {
        let baseline = &pair[0];
        let exact = &pair[1];
        if baseline.mode != "baseline"
            || exact.mode != "exact-ref"
            || baseline.task_id != exact.task_id
            || baseline.task_name != exact.task_name
        {
            bail!(
                "invalid baseline/exact-ref pair at task {}",
                baseline.task_id
            )
        }
        let delta = TaskDelta {
            success: u8::from(exact.success) as i8 - u8::from(baseline.success) as i8,
            anchor_recall: exact.anchor_recall as i16 - baseline.anchor_recall as i16,
            visible: exact.visible as i128 - baseline.visible as i128,
            expand: exact.expand as i128 - baseline.expand as i128,
            retries: exact.retries as i128 - baseline.retries as i128,
            fails: exact.fails as i128 - baseline.fails as i128,
            ratc: exact.ratc - baseline.ratc,
            expand_count: exact.expand_count as i128 - baseline.expand_count as i128,
            dangling_refs: exact.dangling_refs as i128 - baseline.dangling_refs as i128,
        };
        pareto_pairs.push(ParetoPair {
            task_id: baseline.task_id.clone(),
            task_name: baseline.task_name.clone(),
            baseline_visible: baseline.visible,
            baseline_ratc: baseline.ratc,
            exact_ref_visible: exact.visible,
            exact_ref_ratc: exact.ratc,
        });
        per_task.push(TaskComparison {
            task_id: baseline.task_id.clone(),
            task_name: baseline.task_name.clone(),
            baseline: baseline.clone(),
            exact_ref: exact.clone(),
            delta_exact_ref_minus_baseline: delta,
        });
    }
    let baseline: Vec<&TaskCostSummary> = source
        .results
        .iter()
        .filter(|row| row.mode == "baseline")
        .collect();
    let exact: Vec<&TaskCostSummary> = source
        .results
        .iter()
        .filter(|row| row.mode == "exact-ref")
        .collect();
    Ok(AbReport {
        schema_version: "tokenzero.pilot-ab-report.v1",
        source_schema_version: source.schema_version,
        suite_version: source.suite_version.clone(),
        seed: source.seed,
        network: source.network.clone(),
        limitation: "Fixed-suite deltas only; this report makes no statistical-significance claim.",
        aggregates: vec![
            aggregate("baseline", &baseline)?,
            aggregate("exact-ref", &exact)?,
        ],
        per_task,
        pareto_pairs,
    })
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn render_csv(report: &AbReport) -> String {
    let mut lines = vec!["record_type,mode,task_id,task_name,task_count,success,success_rate,anchor_recall,ratc,ratc_mean,ratc_median,visible,visible_total,expand,expand_total,expand_count,expand_rate,dangling_refs,dangling_ref_rate,retries,fails,visible_delta,expand_delta,ratc_delta,success_delta,anchor_recall_delta".to_owned()];
    for a in &report.aggregates {
        lines.push(format!("aggregate,{a_mode},,,{task_count},,{success_rate},{anchor_recall},,{ratc_mean},{ratc_median},,{visible_tokens},,{expand_tokens},,{expand_rate},,{dangling_ref_rate},{retries},{fails},,,,,",
            a_mode=a.mode, task_count=a.task_count, success_rate=a.success_rate, anchor_recall=a.anchor_recall,
            ratc_mean=a.ratc_mean, ratc_median=a.ratc_median, visible_tokens=a.visible_tokens,
            expand_tokens=a.expand_tokens, expand_rate=a.expand_rate, dangling_ref_rate=a.dangling_ref_rate,
            retries=a.retries, fails=a.fails));
    }
    for task in &report.per_task {
        for (row, delta) in [
            (&task.baseline, None),
            (&task.exact_ref, Some(&task.delta_exact_ref_minus_baseline)),
        ] {
            lines.push(format!("task,{mode},{task_id},{task_name},,{success},,{anchor_recall},{ratc},,,{visible},,{expand},,{expand_count},{expand_rate},{dangling_refs},{dangling_rate},{retries},{fails},{visible_delta},{expand_delta},{ratc_delta},{success_delta},{anchor_delta}",
                mode=row.mode, task_id=row.task_id, task_name=csv_cell(&row.task_name), success=u8::from(row.success),
                anchor_recall=row.anchor_recall, ratc=row.ratc, visible=row.visible, expand=row.expand,
                expand_count=row.expand_count, expand_rate=ratio(row.expand_count,row.read_count), dangling_refs=row.dangling_refs,
                dangling_rate=ratio(row.dangling_refs,row.expand_count), retries=row.retries, fails=row.fails,
                visible_delta=delta.map(|d| d.visible.to_string()).unwrap_or_default(),
                expand_delta=delta.map(|d| d.expand.to_string()).unwrap_or_default(),
                ratc_delta=delta.map(|d| d.ratc.to_string()).unwrap_or_default(),
                success_delta=delta.map(|d| d.success.to_string()).unwrap_or_default(),
                anchor_delta=delta.map(|d| d.anchor_recall.to_string()).unwrap_or_default()));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => println!("{}", serde_json::to_string_pretty(&run()?)?),
        [command] if command == "pilot" => println!("{}", serde_json::to_string_pretty(&run()?)?),
        [command, json_path, csv_path] if command == "pilot-report" => {
            let report = build_ab_report(&run()?)?;
            fs::write(
                json_path,
                format!("{}\n", serde_json::to_string_pretty(&report)?),
            )
            .with_context(|| format!("write report JSON {json_path}"))?;
            fs::write(csv_path, render_csv(&report))
                .with_context(|| format!("write report CSV {csv_path}"))?;
        }
        _ => bail!(
            "usage: cargo run -p tokenzero-xtask -- pilot | pilot-report <report.json> <report.csv>"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_frozen_and_all_hashes_are_pinned() {
        let manifest = load_manifest().unwrap();
        assert_eq!(sha256_bytes(MANIFEST.as_bytes()), FROZEN_MANIFEST_SHA256);
        assert_eq!(manifest.tasks.len(), 14);
        for task in &manifest.tasks {
            assert!(!task.success_predicate.is_empty() && !task.anchors.is_empty());
            assert_eq!(task.demand_count, task.expected_expand_count);
            load_task(task).unwrap();
        }
    }

    #[test]
    fn policy_stress_fixtures_exceed_the_default_exact_ref_threshold() {
        let manifest = load_manifest().unwrap();
        for id in [3_u8, 4, 9, 11] {
            let task = &manifest.tasks[id as usize - 1];
            let bytes = fs::metadata(fixture_root().join(&task.fixtures[0].path))
                .unwrap()
                .len() as usize;
            assert!(
                bytes > DEFAULT_CAPSULE_EXACT_REF_THRESHOLD_BYTES,
                "task {id} fixture is only {bytes} bytes"
            );
        }
    }

    #[test]
    fn deterministic_twenty_eight_rows() {
        let first = run().unwrap();
        let second = run().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.results.len(), 28);
        for (index, result) in first.results.iter().enumerate() {
            assert_eq!(result.task_id, (index / 2 + 1).to_string());
            assert_eq!(
                result.mode,
                if index % 2 == 0 {
                    "baseline"
                } else {
                    "exact-ref"
                }
            );
        }
    }

    #[test]
    fn both_modes_use_real_engine_and_succeed() {
        let report = run().unwrap();
        let failures: Vec<String> = report
            .results
            .iter()
            .filter(|result| {
                result.engine != "tokenzero-engine::TokenZeroEngine"
                    || !result.success
                    || result.anchor_recall != 1
            })
            .map(|result| {
                let mut reasons = Vec::new();
                if result.engine != "tokenzero-engine::TokenZeroEngine" {
                    reasons.push(format!("engine={}", result.engine));
                }
                if !result.success {
                    reasons.push("success=false".to_owned());
                }
                if result.anchor_recall != 1 {
                    reasons.push(format!("anchor_recall={}", result.anchor_recall));
                }
                format!(
                    "task_id={} task_name={} mode={} reasons=[{}] visible={} expand={} retries={} fails={} ratc={} expand_count={} dangling_refs={} read_count={}",
                    result.task_id,
                    result.task_name,
                    result.mode,
                    reasons.join(","),
                    result.visible,
                    result.expand,
                    result.retries,
                    result.fails,
                    result.ratc,
                    result.expand_count,
                    result.dangling_refs,
                    result.read_count
                )
            })
            .collect();
        assert!(
            failures.is_empty(),
            "pilot failures:\n{}",
            failures.join("\n")
        );
        let manifest = load_manifest().unwrap();
        for pair in report.results.chunks_exact(2) {
            assert_eq!(pair[0].expand_count, 0);
            assert_eq!(
                pair[1].expand_count,
                manifest.tasks[pair[1].task_id.parse::<usize>().unwrap() - 1].expected_expand_count
                    as u64
            );
        }
    }

    #[test]
    fn stress_and_typed_scenarios_are_behavioral() {
        let report = run().unwrap();
        let exact = |id: &str| {
            report
                .results
                .iter()
                .find(|result| result.task_id == id && result.mode == "exact-ref")
                .unwrap()
        };
        assert_eq!(exact("4").read_count, 8);
        assert_eq!(exact("9").read_count, 6);
        assert_eq!((exact("9").fails, exact("9").dangling_refs), (1, 1));
        assert_eq!(exact("11").read_count, 10);
        assert_eq!(exact("14").fails, 1);
        assert_eq!(exact("10").fails, 0);
        assert_eq!(exact("12").fails, 0);
        assert_eq!(exact("13").expand_count, 0);
    }

    #[test]
    fn eviction_uses_a_real_evicted_ordinal_alias() {
        let manifest = load_manifest().unwrap();
        let (_, payload) = load_task(&manifest.tasks[8]).unwrap();
        let outcome = run_eviction_stress(&payload).unwrap();
        assert_eq!(outcome.fails, 1);
        assert_eq!(outcome.dangling_refs, 1);
        assert!(outcome.recovered.contains("EVICT_0"));
        assert!(outcome.recovered.contains("EVICT_5"));
        assert!(outcome.recovered.contains("dangling-ref"));
    }

    #[test]
    fn golden_sample_report_matches_json_and_csv() {
        let sample = |mode, visible, expand, ratc, expand_count| TaskCostSummary {
            record_type: "task-cost-summary",
            task_id: "1".to_owned(),
            task_name: "sample".to_owned(),
            mode,
            engine: "tokenzero-engine::TokenZeroEngine",
            seed: 7,
            success: true,
            visible,
            expand,
            retries: 0,
            fails: 0,
            ratc,
            expand_count,
            dangling_refs: 0,
            anchor_recall: 1,
            read_count: 2,
        };
        let source = Report {
            schema_version: "tokenzero.task-cost-summary.v2",
            suite_version: "golden-v1".to_owned(),
            seed: 7,
            network: "disabled".to_owned(),
            results: vec![
                sample("baseline", 100, 0, 100.0, 0),
                sample("exact-ref", 40, 10, 50.0, 1),
            ],
        };
        let report = build_ab_report(&source).unwrap();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schema_version"], "tokenzero.pilot-ab-report.v1");
        assert_eq!(value["aggregates"][0]["success_rate"], 1.0);
        assert_eq!(value["aggregates"][1]["expand_rate"], 0.5);
        assert_eq!(
            value["per_task"][0]["delta_exact_ref_minus_baseline"]["visible"],
            -60
        );
        assert_eq!(
            value["per_task"][0]["delta_exact_ref_minus_baseline"]["ratc"],
            -50.0
        );
        assert_eq!(value["pareto_pairs"][0]["exact_ref_visible"], 40);
        let csv = render_csv(&report);
        assert!(csv.starts_with("record_type,mode,task_id,task_name"));
        assert!(csv.contains("aggregate,baseline"));
        assert!(csv.contains("aggregate,exact-ref"));
        assert!(csv.contains("task,baseline,1,sample"));
        assert!(csv.contains("task,exact-ref,1,sample"));
        assert!(csv.contains(",-60,10,-50,0,0\n"));
        let widths: Vec<usize> = csv.lines().map(|line| line.split(',').count()).collect();
        assert!(widths.iter().all(|width| *width == widths[0]), "{widths:?}");
    }

    #[test]
    fn full_ab_report_is_paired_complete_and_deterministic() {
        let source = run().unwrap();
        let first = build_ab_report(&source).unwrap();
        let second = build_ab_report(&source).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.aggregates.len(), 2);
        assert_eq!(first.per_task.len(), 14);
        assert_eq!(first.pareto_pairs.len(), 14);
        assert_eq!(first.aggregates[0].task_count, 14);
        assert_eq!(first.aggregates[1].task_count, 14);
        assert_eq!(first.aggregates[0].success_rate, 1.0);
        assert_eq!(first.aggregates[1].success_rate, 1.0);
        assert!(
            first
                .per_task
                .iter()
                .all(|task| task.baseline.mode == "baseline" && task.exact_ref.mode == "exact-ref")
        );
    }

    #[test]
    fn rows_are_task_cost_summary_compatible() {
        let value = serde_json::to_value(run().unwrap()).unwrap();
        for row in value["results"].as_array().unwrap() {
            for field in [
                "task_id",
                "success",
                "visible",
                "expand",
                "retries",
                "fails",
                "ratc",
                "expand_count",
                "dangling_refs",
                "mode",
                "anchor_recall",
                "read_count",
            ] {
                assert!(row.get(field).is_some(), "missing {field}");
            }
            assert!(row["task_id"].is_string());
            assert!(row["success"].is_boolean());
            assert!(row["ratc"].is_number());
        }
    }
}
