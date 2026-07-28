"""tokenzero-b452: canonical dispatch authorization (marker-based slicing)."""

P = "crates/tokenzero-mcp/src/codemode/exec.rs"
lines = open(P).read().split("\n")

def find(marker, start=0):
    for i in range(start, len(lines)):
        if marker in lines[i]:
            return i
    raise SystemExit("marker not found: " + marker[:60])

def block_end(i, indent):
    # first line after i that equals indent + "}"
    for j in range(i + 1, len(lines)):
        if lines[j] == indent + "}":
            return j
    raise SystemExit("no block end")

def drop(a, b, also_blank=True):
    del lines[a:b + 1]
    if also_blank and a < len(lines) and lines[a].strip() == "":
        del lines[a]

# 1. recipe-source lexical denial (~2702)
i = find("if quickjs_plan_requests_mutation(&source) {")
drop(i, block_end(i, "        "))

# 2. lexical pre-denial in execute_code
i = find("if use_quickjs && quickjs_plan_requests_mutation(plan) {")
e = block_end(i, "    ")
lines[i:e + 1] = [
    "    // Mutation denial is enforced at the canonical dispatch boundary",
    "    // (begin_js_host_op) from resolved effect metadata, not by scanning plan",
    "    // source (tokenzero-b452).",
]

# 3. classifier fn
i = find("fn quickjs_plan_requests_mutation(plan: &str) -> bool {")
drop(i, block_end(i, ""))

# 4. classifier test module
i = find("mod quickjs_mutation_classifier_tests {")
# the #[cfg(test)] attribute line sits directly above
attr = i - 1
assert "cfg(test)" in lines[attr], lines[attr]
drop(attr, block_end(i, ""))

open(P, "w").write("\n".join(lines))
print("exec.rs phase1 ok")
