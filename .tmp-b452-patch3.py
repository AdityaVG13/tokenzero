"""b452 phase 3: scrub string literals before advisory classification."""
import subprocess

P = "crates/tokenzero-mcp/src/codemode/containment.rs"
s = open(P).read()

SCRUB = r'''
/// Blank out string/template literal contents (preserving newlines) so quoted
/// prose is never read as a work-class signal. The scan below is advisory
/// scheduling only; authorization lives at the canonical dispatch boundary
/// (tokenzero-b452).
fn scrub_plan_literals(plan: &str) -> String {
    let mut scrubbed = String::with_capacity(plan.len());
    let mut quote = None;
    let mut escaped = false;

    for ch in plan.chars() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            scrubbed.push(if ch == '\n' { '\n' } else { ' ' });
        } else if matches!(ch, '\'' | '"' | 'BT') {
            quote = Some(ch);
            scrubbed.push(' ');
        } else {
            scrubbed.push(ch);
        }
    }

    scrubbed
}

'''

bt = subprocess.run(["printf", "\\140"], capture_output=True, text=True).stdout
SCRUB = SCRUB.replace("BT", bt)

anchor = "fn classify(plan: &str, cost_threshold: usize) -> ExecutionClass {\n    let p = plan.trim().to_ascii_lowercase();\n"
assert s.count(anchor) == 1, s.count(anchor)
s = s.replace(anchor, SCRUB + "fn classify(plan: &str, cost_threshold: usize) -> ExecutionClass {\n    let scrubbed = scrub_plan_literals(plan);\n    let p = scrubbed.trim().to_ascii_lowercase();\n")

open(P, "w").write(s)
print("phase3 ok")
