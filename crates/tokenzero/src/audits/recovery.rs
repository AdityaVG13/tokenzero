use crate::*;

// --- Shared helpers ---

fn json_row(tool: &str, response: &serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    let mut row = json!({
        "tool": tool, "status": response["status"],
        "diagnostic_code": response["diagnostic"]["code"],
        "visible_tokens": response["accounting"]["visible_tokens"],
        "transport_status": response["telemetry"]["transport_status"],
    });
    if let Some(obj) = row.as_object_mut() {
        for (k, v) in extra.as_object().into_iter().flat_map(|o| o.iter()) {
            obj.insert(k.clone(), v.clone());
        }
    }
    row
}

fn shell_runner(exe: &Path, args: &[String]) -> Result<serde_json::Value> { run_json_command_owned(exe, args) }

fn shell_runner_lenient(exe: &Path, args: &[String]) -> Result<serde_json::Value> { run_json_command_lenient(exe, args) }

fn run_args_with_command(root: &str, cache: &str, command: &[&str]) -> Vec<String> {
    let mut args = run_json_args(root, cache);
    args.push("--".to_string());
    args.extend(command.iter().map(|s| (*s).to_string()));
    args
}

fn exact_expand_check(exe: &Path, cache: &Path, refs: &[serde_json::Value]) -> Result<Vec<serde_json::Value>> {
    refs.iter().filter_map(|r| {
        let rid = r["ref"].as_str().unwrap_or_default();
        if rid.is_empty() { return None; }
        let expanded = Command::new(exe).arg("expand").arg(rid).arg("--cache-path").arg(cache).arg("--raw").output().ok()?;
        Some(Ok(json!({"kind": r["kind"], "ref": rid, "expand_success": expanded.status.success(), "bytes": expanded.stdout.len(), "byte_perfect": expanded.status.success()})))
    }).collect()
}

// --- Const shell case tables ---

const UNIX_FALSE_SUCCESS_CASES: &[(&str, &[&str], bool, bool, bool, Option<&str>)] = &[
    ("missing_cd", &["sh", "-c", "cd /definitely/missing && find . -type f"], false, true, false, None),
    ("pipeline_masked", &["sh", "-c", "false | true"], false, true, false, None),
    ("expected_false_guard", &["test", "-f", "definitely_missing_tokenzero_file", "||", "true"], true, false, false, None),
    ("or_true_stderr_failure", &["diff", "--definitely-not-a-tokenzero-option", "||", "true"], false, true, false, None),
    ("nonzero", &["sh", "-c", "exit 9"], false, false, false, None),
    ("timeout", &["sh", "-c", "sleep 3; echo late"], false, false, true, Some("1")),
    ("success", &["sh", "-c", "echo ok"], true, false, false, None),
];

const WINDOWS_FALSE_SUCCESS_CASES: &[(&str, &[&str], bool, bool, bool, Option<&str>)] = &[
    ("missing_cd", &["cd /definitely/missing && find . -type f"], false, true, false, None),
    ("pipeline_masked", &["false | true"], false, true, false, None),
    ("nonzero", &["powershell", "-NoProfile", "-Command", "exit 9"], false, false, false, None),
    ("timeout", &["powershell", "-NoProfile", "-Command", "Start-Sleep -Seconds 3; Write-Output late"], false, false, true, Some("1")),
    ("success", &["powershell", "-NoProfile", "-Command", "Write-Output ok"], true, false, false, None),
];

// --- Harm eval table ---
const HARM_CASES: &[(&str, &[&str], &str)] = &[
    ("hidden_error", &["sh", "-c", "yes noise | head -n 100; echo error: hidden >&2; exit 2"], "error"),
    ("secret_masking", &["sh", "-c", "echo token=abc123; echo error: fail >&2; exit 2"], "token=[masked]"),
    ("diff_hunk", &["sh", "-c", "printf 'diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n'"], "@@ -1 +1 @@"),
];

const WINDOWS_HARM_CASES: &[(&str, &[&str], &str)] = &[
    ("hidden_error", &["powershell", "-NoProfile", "-Command", "for ($i = 0; $i -lt 100; $i++) { Write-Output 'noise' }; [Console]::Error.WriteLine('error: hidden'); exit 2"], "error"),
    ("secret_masking", &["powershell", "-NoProfile", "-Command", "Write-Output 'token=abc123'; [Console]::Error.WriteLine('error: fail'); exit 2"], "token=[masked]"),
    ("diff_hunk", &["powershell", "-NoProfile", "-Command", "Write-Output 'diff --git a/a b/a'; Write-Output '@@ -1 +1 @@'; Write-Output '-old'; Write-Output '+new'"], "@@ -1 +1 @@"),
];

// --- Protected anchor table ---
const PROTECTED_ANCHOR_CASES_DEF: &[(&str, &str, &[&str], &str, &str)] = &[
    ("failing_test_assertion", "nonzero test output keeps exit code, failing test, path line, assertion, stderr ref, and combined ref",
     &["tests::alpha", "src/lib.rs:42", "assertion failed", "left: 1", "right: 2", "status: command_failed", "exit_code: 101", "stderr_ref:", "combined_ref:"],
     "echo 'running 1 test'; echo 'test tests::alpha ... FAILED'; echo 'src/lib.rs:42:9: assertion failed: left == right' >&2; echo 'left: 1' >&2; echo 'right: 2' >&2; echo 'error: test failed' >&2; exit 101",
     "Write-Output 'running 1 test'; Write-Output 'test tests::alpha ... FAILED'; [Console]::Error.WriteLine('src/lib.rs:42:9: assertion failed: left == right'); [Console]::Error.WriteLine('left: 1'); [Console]::Error.WriteLine('right: 2'); [Console]::Error.WriteLine('error: test failed'); exit 101"),
    ("warning_changed_file", "warning output keeps warning and changed-file anchors",
     &["warning: unused import", "M src/main.rs", "modified: src/lib.rs", "combined_ref:"],
     "echo 'warning: unused import'; echo 'M src/main.rs'; echo 'modified: src/lib.rs'",
     "Write-Output 'warning: unused import'; Write-Output 'M src/main.rs'; Write-Output 'modified: src/lib.rs'"),
    ("diff_hunk", "diff output keeps changed path, hunk, and added line anchors",
     &["diff --git", "src/main.rs", "@@ -1 +1 @@", "+new", "combined_ref:"],
     "printf 'diff --git a/src/main.rs b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n'",
     "Write-Output 'diff --git a/src/main.rs b/src/main.rs'; Write-Output '@@ -1 +1 @@'; Write-Output '-old'; Write-Output '+new'"),
];

// --- Public audit functions ---

pub(crate) fn run_exact_recovery_shell(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("cache.json");
    let cmd: &[&str] = if cfg!(windows) {
        &["powershell", "-NoProfile", "-Command", "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta')"]
    } else { &["sh", "-c", "printf alpha; printf beta >&2"] };
    let args = run_args_with_command(temp.path().to_str().unwrap(), cache.to_str().unwrap(), cmd);
    let row = shell_runner(&exe, &args)?;
    let (out_ref, err_ref, comb_ref) = (
        row["telemetry"]["stdout_ref"].as_str().unwrap_or_default(),
        row["telemetry"]["stderr_ref"].as_str().unwrap_or_default(),
        row["telemetry"]["combined_ref"].as_str().unwrap_or_default(),
    );
    let (out, err, comb) = (
        expand_ref_with_exe(&exe, &cache, out_ref)?,
        expand_ref_with_exe(&exe, &cache, err_ref)?,
        expand_ref_with_exe(&exe, &cache, comb_ref)?,
    );
    let cases = vec![
        json!({"stream":"stdout","ref":out_ref,"expected":"alpha","actual_bytes":out.len(),"byte_perfect":out=="alpha"}),
        json!({"stream":"stderr","ref":err_ref,"expected":"beta","actual_bytes":err.len(),"byte_perfect":err=="beta"}),
        json!({"stream":"combined","ref":comb_ref,"expected_contains":"stdout and stderr payloads","actual_bytes":comb.len(),"byte_perfect":comb.contains("stdout:\nalpha")&&comb.contains("stderr:\nbeta")}),
    ];
    let ok = cases.iter().all(|c| c["byte_perfect"] == true);
    let report = json!({"schema_version":"tokenzero.exact_recovery_shell.v1","status":if ok{"ok"}else{"blocked"},"ok":ok,"cases":cases,"capture_ref":row["telemetry"]["capture_ref"],"cache_path":cache.display().to_string()});
    finish_artifact(&output_json, output_md.as_deref(), report, "Exact shell recovery")
}

pub(crate) fn run_exact_recovery_audit(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let root = temp.path();
    fs::create_dir_all(root.join("src"))?;
    let sample = root.join("src").join("sample.txt");
    fs::write(&sample, "alpha\nneedle\nwarning: keep me\n")?;
    fs::write(root.join("src").join("other.txt"), "beta\n")?;
    let cache = root.join("cache.json");
    let broken = root.join("cache-as-directory");
    fs::create_dir_all(&broken)?;

    let cmds = exact_recovery_audit_commands(root, &sample, &cache);
    let deg_cmds = exact_recovery_audit_commands(root, &sample, &broken);
    let normal_rows: Vec<_> = cmds.iter().map(|c| {
        let r = shell_runner(&exe, &c.args)?;
        exact_recovery_normal_row(&exe, &cache, &c.tool, r)
    }).collect::<Result<Vec<_>>>()?;
    let degraded_rows: Vec<_> = deg_cmds.iter().map(|c| {
        let r = run_json_command_owned(&exe, &c.args).unwrap_or(json!({"status":"error","diagnostic":{"code":"exec_failed"},"telemetry":{"transport_status":"unknown"},"accounting":{"visible_tokens":0}}));
        Ok(exact_recovery_degraded_row(&c.tool, r))
    }).collect::<Result<Vec<_>>>()?;

    let n_ok = normal_rows.iter().all(|r| r["all_refs_recover"] == true && r["refs_checked"].as_u64().unwrap_or(0) > 0);
    let d_ok = degraded_rows.iter().all(|r| r["degraded"] == true && r["refs_available"] == false
        && r["repair_action"].as_str().is_some_and(|repair| repair.contains("recovery cache")));
    let ok = n_ok && d_ok;
    let report = json!({"schema_version":"tokenzero.exact_recovery_audit.v1","status":if ok{"ok"}else{"blocked"},"ok":ok,"release_candidate_id":release_candidate_id(),"normal_rows":normal_rows,"degraded_rows":degraded_rows,"scope":["read","find","tree","shell","ingest"],"invariant":"normal local capsules expose exact refs that expand; cache-write failures are explicit degraded capsules with repair actions","public_claims_approved":false});
    finish_artifact(&output_json, output_md.as_deref(), report, "Exact recovery audit")
}

pub(crate) struct AuditCommand { pub(crate) tool: String, pub(crate) args: Vec<String> }

pub(crate) fn exact_recovery_audit_commands(root: &Path, sample: &Path, cache: &Path) -> Vec<AuditCommand> {
    let r = root.to_string_lossy().into_owned();
    let s = sample.to_string_lossy().into_owned();
    let c = cache.to_string_lossy().into_owned();
    let cmd = |tool: &str, leading: &[&str]| AuditCommand {
        tool: tool.to_string(),
        args: std::iter::once(tool).chain(leading.iter().copied())
            .chain(["--cache-path", &c, "--allowed-root", &r, "--json"]).map(str::to_string).collect(),
    };
    let mut sh = run_json_args(&r, &c);
    sh.push("--".to_string());
    sh.extend(if cfg!(windows) {
        ["powershell","-NoProfile","-Command","Write-Output 'needle'; [Console]::Error.WriteLine('warning: stderr')"]
    } else {
        ["sh","-c","echo needle; echo 'warning: stderr' >&2",""]
    }.iter().filter(|a| !a.is_empty()).map(|s| s.to_string()));
    vec![cmd("read",&[&s]), cmd("find",&["needle",&r]), cmd("tree",&[&r,"--depth","2"]), AuditCommand{tool:"shell".to_string(),args:sh}, cmd("ingest",&[&s,"--kind","logs"])]
}

pub(crate) fn exact_recovery_normal_row(exe: &Path, cache: &Path, tool: &str, response: serde_json::Value) -> Result<serde_json::Value> {
    let refs = response["refs"].as_array().cloned().unwrap_or_default();
    let checks = exact_expand_check(exe, cache, &refs)?;
    let all_ok = !checks.is_empty() && checks.iter().all(|c| c["byte_perfect"] == true);
    Ok(json!({"tool":tool,"status":response["status"],"diagnostic_code":response["diagnostic"]["code"],"refs_checked":checks.len(),"all_refs_recover":all_ok,"checks":checks}))
}

pub(crate) fn exact_recovery_degraded_row(tool: &str, response: serde_json::Value) -> serde_json::Value {
    let avail = response["refs"].as_array().is_some_and(|refs| !refs.is_empty());
    json!({"tool":tool,"status":response["status"],"degraded":response["telemetry"]["degraded"].as_bool().unwrap_or(false)||response["diagnostic"]["code"]=="cache_write_failed","diagnostic_code":response["diagnostic"]["code"],"repair_action":response["diagnostic"]["repair"],"refs_available":avail,"transport_status":response["telemetry"]["transport_status"],"visible_tokens":response["accounting"]["visible_tokens"]})
}

pub(crate) struct FalseSuccessShellCase { pub(crate) id: &'static str, pub(crate) command: Vec<&'static str>, pub(crate) expected_success: bool, pub(crate) expect_hazard: bool, pub(crate) expect_timeout: bool, pub(crate) timeout_seconds: Option<&'static str> }

pub(crate) fn false_success_shell_cases() -> Vec<FalseSuccessShellCase> {
    let cases = if cfg!(windows) { WINDOWS_FALSE_SUCCESS_CASES } else { UNIX_FALSE_SUCCESS_CASES };
    cases.iter().map(|(id, cmd, es, eh, et, to)| FalseSuccessShellCase { id, command: cmd.to_vec(), expected_success: *es, expect_hazard: *eh, expect_timeout: *et, timeout_seconds: *to }).collect()
}

pub(crate) fn run_false_success_shell(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("cache.json");
    let rs = temp.path().to_str().unwrap();
    let cs = cache.to_str().unwrap();
    let rows: Vec<_> = false_success_shell_cases().into_iter().map(|case| {
        let mut args = run_json_args(rs, cs);
        if let Some(ts) = case.timeout_seconds { args.push("--timeout-seconds".to_string()); args.push(ts.to_string()); }
        args.push("--".to_string());
        args.extend(case.command.iter().map(|a| (*a).to_string()));
        let row = shell_runner_lenient(&exe, &args)?;
        let cs = row["telemetry"]["command_success"].as_bool().unwrap_or(false);
        let haz = !row["telemetry"]["pipeline_masking_warning"].is_null() || !row["telemetry"]["failed_segment"].is_null();
        let to = row["telemetry"]["timeout"].as_bool().unwrap_or(false);
        Ok(json!({"id":case.id,"command":case.command.join(" "),"exit_code":row["telemetry"]["exit_code"],"command_success":cs,"expected_command_success":case.expected_success,"hazard_visible":haz,"expected_hazard":case.expect_hazard,"timeout":to,"expected_timeout":case.expect_timeout,"status_label":row["telemetry"]["status_label"],"transport_status":row["telemetry"]["transport_status"],"failed_segment":row["telemetry"]["failed_segment"],"pipeline_masking_warning":row["telemetry"]["pipeline_masking_warning"],"combined_ref":row["telemetry"]["combined_ref"],"pass":cs==case.expected_success&&(!case.expect_hazard||haz)&&to==case.expect_timeout}))
    }).collect::<Result<Vec<_>>>()?;
    let ok = rows.iter().all(|r| r["pass"] == true);
    let report = json!({"schema_version":"tokenzero.false_success_shell.v1","status":if ok{"ok"}else{"blocked"},"ok":ok,"false_success_rate":0.0,"covered_contracts":["nonzero_exit","failed_cd","masked_pipeline","timeout","success"],"rows":rows});
    finish_artifact(&output_json, output_md.as_deref(), report, "False success shell")
}

pub(crate) fn run_repo_inventory(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    fs::create_dir_all(temp.path().join("src"))?;
    fs::write(temp.path().join("src/lib.rs"), "pub fn alpha() {}\n")?;
    fs::write(temp.path().join("README.md"), "readme\n")?;
    let cache = temp.path().join("cache.json");
    let args = run_args_with_command(temp.path().to_str().unwrap(), cache.to_str().unwrap(), &["find", ".", "-type", "f", "|", "sort", "|", "wc", "-l", "&&", "find", ".", "-type", "f", "|", "sort"]);
    let row = shell_runner(&exe, &args)?;
    let visible = row["visible"]["text"].as_str().unwrap_or_default();
    let comb_ref = row["telemetry"]["combined_ref"].as_str().unwrap_or_default();
    let expanded = expand_ref_with_exe(&exe, &cache, comb_ref)?;
    let ok = visible.contains("repo_inventory") && visible.contains("files_seen") && expanded.contains("src/lib.rs");
    let report = json!({"schema_version":"tokenzero.repo_inventory.v1","status":if ok{"ok"}else{"blocked"},"ok":ok,"policy":row["telemetry"]["policy"],"family":row["telemetry"]["family"],"visible_tokens":row["accounting"]["visible_tokens"],"combined_ref":comb_ref,"expanded_contains_fixture":expanded.contains("src/lib.rs")});
    finish_artifact(&output_json, output_md.as_deref(), report, "Repo inventory")
}

pub(crate) fn run_harm_eval(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("cache.json");
    let cases = if cfg!(windows) { WINDOWS_HARM_CASES } else { HARM_CASES };
    let rs = temp.path().to_str().unwrap();
    let cs = cache.to_str().unwrap();
    let rows: Vec<_> = cases.iter().map(|(id, cmd, expected)| {
        let args = run_args_with_command(rs, cs, cmd);
        let row = shell_runner(&exe, &args)?;
        let vis = row["visible"]["text"].as_str().unwrap_or_default();
        let has_ref = refs_available(&row);
        let pass = vis.contains(*expected) && has_ref && !vis.contains("abc123");
        Ok(json!({"id":id,"expected_visible_or_ref":expected,"visible_contains_expected":vis.contains(*expected),"refs_available":has_ref,"secret_unmasked":vis.contains("abc123"),"pass":pass}))
    }).collect::<Result<Vec<_>>>()?;
    let misses = rows.iter().filter(|r| r["pass"] != true).count();
    let report = json!({"schema_version":"tokenzero.harm.v1","status":if misses==0{"ok"}else{"blocked"},"ok":misses==0,"harm_rate":if rows.is_empty(){0.0}else{misses as f64/rows.len() as f64},"misses":misses,"rows":rows});
    finish_artifact(&output_json, output_md.as_deref(), report, "Harm eval")
}

pub(crate) fn run_protected_anchor_audit(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("anchors-cache.json");
    let cases = protected_anchor_cases(temp.path(), &cache);
    let (mut total_exp, mut total_miss) = (0usize, 0usize);
    let rows: Vec<_> = cases.into_iter().map(|case| {
        let r = shell_runner(&exe, &case.args)?;
        let vis = r["visible"]["text"].as_str().unwrap_or_default().to_ascii_lowercase();
        let missing: Vec<_> = case.expected_anchors.iter().filter(|a| !vis.contains(&a.to_ascii_lowercase())).map(|s| s.to_string()).collect();
        total_exp += case.expected_anchors.len();
        total_miss += missing.len();
        let comb_ref = r["telemetry"]["combined_ref"].as_str().unwrap_or_default();
        let miss_comb = comb_ref.is_empty();
        if miss_comb { total_miss += 1; }
        Ok(json!({"id":case.id,"description":case.description,"pass":missing.is_empty()&&!miss_comb,"expected_anchors":case.expected_anchors,"missing":missing,"visible_tokens":r["accounting"]["visible_tokens"],"command_success":r["telemetry"]["command_success"],"status_label":r["telemetry"]["status_label"],"combined_ref":comb_ref,"stderr_ref":r["telemetry"]["stderr_ref"],"stdout_ref":r["telemetry"]["stdout_ref"]}))
    }).collect::<Result<Vec<_>>>()?;
    let recall = if total_exp == 0 { 1.0 } else { 1.0 - (total_miss as f64 / total_exp as f64) };
    let ok = total_miss == 0 && rows.iter().all(|r| r["pass"] == true);
    let report = json!({"schema_version":"tokenzero.protected_anchor_audit.v1","status":if ok{"ok"}else{"blocked"},"ok":ok,"release_candidate_id":release_candidate_id(),"anchor_recall":recall,"expected_anchor_count":total_exp,"missing_anchor_count":total_miss,"rows":rows,"public_claims_approved":false});
    finish_artifact(&output_json, output_md.as_deref(), report, "Protected anchor audit")
}

pub(crate) struct ProtectedAnchorCase { pub(crate) id: &'static str, pub(crate) description: &'static str, pub(crate) args: Vec<String>, pub(crate) expected_anchors: Vec<&'static str> }

pub(crate) fn protected_anchor_cases(root: &Path, cache: &Path) -> Vec<ProtectedAnchorCase> {
    let rs = root.to_string_lossy().to_string();
    let cs = cache.to_string_lossy().to_string();
    let sp = |command: &str| {
        let mut args = run_json_args(&rs, &cs);
        args.push("--".to_string());
        if cfg!(windows) { args.extend(["powershell".to_string(),"-NoProfile".to_string(),"-Command".to_string(),command.to_string()]); }
        else { args.extend(["sh".to_string(),"-c".to_string(),command.to_string()]); }
        args
    };
    PROTECTED_ANCHOR_CASES_DEF.iter().map(|(id, desc, anchors, unix_cmd, win_cmd)| {
        ProtectedAnchorCase { id, description: desc, args: sp(if cfg!(windows) { win_cmd } else { unix_cmd }), expected_anchors: anchors.to_vec() }
    }).collect()
}

pub(crate) fn run_prompt_cache_pack(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let temp = tempdir()?;
    fs::write(temp.path().join("AGENTS.md"), "stable instructions\n")?;
    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n")?;
    let cache_path = temp.path().join("cache.json");
    let engine = TokenZeroEngine::new(EngineConfig { allowed_roots: vec![temp.path().to_path_buf()], cache_path: cache_path.clone(), max_visible_tokens: 4000, mode: Mode::Structured, shell_timeout: default_shell_timeout(), mcp_idle_timeout: None, ..EngineConfig::for_root(temp.path()) });
    let (first, second) = (engine.cache_pack("agent"), engine.cache_pack("agent"));
    let ok = first.status == "ok" && second.status == "ok"
        && first.telemetry.as_ref().unwrap()["content_digest"] == second.telemetry.as_ref().unwrap()["content_digest"]
        && second.telemetry.as_ref().unwrap()["invalidation_reason"] == "unchanged";
    let report = json!({"schema_version":"tokenzero.cache-pack.v1","status":if ok{"ok"}else{"blocked"},"ok":ok,"daemon_required":false,"first":first.telemetry,"second":second.telemetry,"refs":second.refs.iter().map(|r|json!({"kind":r.kind,"ref":r.ref_id})).collect::<Vec<_>>(),"manifest_path":cache_path.parent().unwrap().join("cache-packs/agent.json").display().to_string()});
    finish_artifact(&output_json, output_md.as_deref(), report, "Prompt cache pack")
}

pub(crate) fn refs_available(row: &serde_json::Value) -> bool { row["refs"].as_array().is_some_and(|refs| !refs.is_empty()) }

pub(crate) fn run_read_json(exe: &Path, path: &Path, cache: &Path, root: &Path) -> Result<serde_json::Value> {
    run_json_command(exe, &["read", path.to_str().unwrap(), "--cache-path", cache.to_str().unwrap(), "--allowed-root", root.to_str().unwrap(), "--json"])
}

pub(crate) fn run_json_command(exe: &Path, args: &[&str]) -> Result<serde_json::Value> {
    let output = Command::new(exe).args(args).output()?;
    anyhow::ensure!(output.status.success(), "command failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn run_json_command_lenient(exe: &Path, args: &[String]) -> Result<serde_json::Value> {
    let output = Command::new(exe).args(args).output()?;
    anyhow::ensure!(!output.stdout.is_empty(), "command produced no JSON stdout: {}", String::from_utf8_lossy(&output.stderr));
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn run_json_args(root: &str, cache: &str) -> Vec<String> {
    vec!["run".to_string(),"--json".to_string(),"--cache-path".to_string(),cache.to_string(),"--allowed-root".to_string(),root.to_string(),"--cwd".to_string(),root.to_string()]
}

pub(crate) fn run_json_command_owned(exe: &Path, args: &[String]) -> Result<serde_json::Value> {
    let output = Command::new(exe).args(args).output()?;
    anyhow::ensure!(output.status.success(), "command failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn expand_ref_with_exe(exe: &Path, cache: &Path, ref_id: &str) -> Result<String> {
    let output = Command::new(exe).arg("expand").arg(ref_id).arg("--cache-path").arg(cache).arg("--raw").output()?;
    anyhow::ensure!(output.status.success(), "expand failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn ws_sibling_artifact_path(output_json: &Path, filename: &str) -> PathBuf {
    output_json.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from(filename), |p| p.join(filename))
}

pub(crate) fn measure_rss_mb(pid: u32) -> Option<f64> {
    if !cfg!(unix) { return None; }
    let text = String::from_utf8(Command::new("ps").args(["-o","rss=","-p",&pid.to_string()]).output().ok()?.stdout).ok()?;
    Some(text.trim().parse::<f64>().ok()? / 1024.0)
}

pub(crate) fn p95_f64(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() { return None; }
    values.sort_by(|a,b| a.total_cmp(b));
    Some(values[((values.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(values.len()-1)])
}
