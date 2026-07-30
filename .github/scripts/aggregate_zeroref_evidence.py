#!/usr/bin/env python3
"""Fail-closed aggregation for retained native ZeroRef conformance evidence."""
import argparse, copy, hashlib, json, re, tempfile
from pathlib import Path

SCHEMA="zeroref-conformance-evidence/v1"
LIFECYCLE_SCHEMA="zeroref-lifecycle-smokes/v1"
MERGED="zeroref-conformance-merged-evidence/v1"
OSES=("macos","linux","windows")
ENGINES=("fszero","graphzero","tokenzero")
PAYLOADS=("empty","utf8_text","crlf","binary","big")
FRAGMENTS=("B0-5","B6-10","B0-0","L1-1","L2-3")
LIFECYCLES=("fresh","upgraded_legacy","explicit_shared","default_isolated","incompatible_peer","corruption","disable","rollback")
H40=re.compile(r"[0-9a-f]{40}\Z")
H64=re.compile(r"[0-9a-f]{64}\Z")

def need(ok,msg):
    if not ok: raise ValueError(msg)

def documents(root):
    found=[]
    for path in sorted(root.rglob("*.json")):
        try: value=json.loads(path.read_text(encoding="utf-8"))
        except (OSError,json.JSONDecodeError) as error:
            raise ValueError(f"malformed JSON evidence {path}: {error}") from error
        if value.get("schema")==SCHEMA: found.append(value)
    need(found,f"no {SCHEMA} documents found under {root}")
    return found

def lifecycle_documents(root):
    found=[]
    for path in sorted(root.rglob("*.json")):
        try: value=json.loads(path.read_text(encoding="utf-8"))
        except (OSError,json.JSONDecodeError) as error:
            raise ValueError(f"malformed JSON evidence {path}: {error}") from error
        if value.get("schema")==LIFECYCLE_SCHEMA: found.append(value)
    need(found,f"no {LIFECYCLE_SCHEMA} documents found under {root}")
    return found

def validate_lifecycle(doc):
    need(isinstance(doc,dict) and doc.get("schema")==LIFECYCLE_SCHEMA,"unexpected lifecycle schema")
    os_name=doc.get("os")
    need(os_name in OSES,f"unknown lifecycle OS {os_name!r}")
    need(doc.get("status")=="pass",f"{os_name}: lifecycle status is not pass")
    cells=doc.get("cells")
    need(isinstance(cells,list) and len(cells)==len(LIFECYCLES),f"{os_name}: lifecycle cells are incomplete")
    need({c.get("test") for c in cells}==set(LIFECYCLES),f"{os_name}: lifecycle coordinates are incomplete or duplicated")
    need(all(c.get("status")=="pass" and c.get("os")==os_name for c in cells),
         f"{os_name}: lifecycle cell skipped, failed, or mislabeled")
    return os_name

def validate_binary(meta,os_name,expected):
    need(isinstance(meta,dict),f"{os_name}: binary metadata must be an object")
    engine=meta.get("engine")
    need(engine in ENGINES,f"{os_name}: unknown binary engine {engine!r}")
    need(meta.get("os")==os_name,f"{os_name}/{engine}: binary OS mismatch")
    need(isinstance(meta.get("path"),str) and meta["path"],f"{os_name}/{engine}: empty binary path")
    need(isinstance(meta.get("version"),str) and meta["version"],f"{os_name}/{engine}: empty binary version")
    sha=meta.get("sha256")
    need(isinstance(sha,str) and H64.fullmatch(sha),f"{os_name}/{engine}: malformed binary SHA-256")
    commit=meta.get("commit")
    need(isinstance(commit,str) and commit!="unknown" and H40.fullmatch(commit),
         f"{os_name}/{engine}: missing, unknown, or malformed commit")
    need(commit==expected[engine],f"{os_name}/{engine}: unexpected commit {commit}")

def validate(doc,expected):
    need(isinstance(doc,dict) and doc.get("schema")==SCHEMA,"unexpected evidence schema")
    need(doc.get("zeroref_version")=="v1","unexpected ZeroRef version")
    os_name=doc.get("host_os")
    need(os_name in OSES,f"unknown host_os {os_name!r}")
    scope=doc.get("scope")
    need(isinstance(scope,dict) and scope.get("explicit") is True and
         scope.get("kind")=="native-os" and scope.get("os")==os_name,
         f"{os_name}: native scope was not explicit and exact")
    matrix=doc.get("matrix")
    need(isinstance(matrix,dict) and matrix.get("status")=="green",f"{os_name}: native matrix is not green")
    rows=matrix.get("rows")
    need(isinstance(rows,list) and len(rows)==1 and rows[0].get("os")==os_name,
         f"{os_name}: expected exactly one native row")
    cells=rows[0].get("cells")
    expected_cells={(w,r,p) for w in ENGINES for r in ENGINES for p in PAYLOADS}
    need(isinstance(cells,list) and len(cells)==len(expected_cells),f"{os_name}: native cell matrix is empty or incomplete")
    need({(c.get("writer"),c.get("reader"),c.get("payload")) for c in cells}==expected_cells,
         f"{os_name}: native cell coordinates are incomplete or duplicated")
    need(all(c.get("status")=="pass" for c in cells),f"{os_name}: native cell skipped or failed")
    for cell in cells:
        expected_hash=cell.get("expected_hash"); actual_hash=cell.get("actual_hash")
        need(isinstance(expected_hash,str) and H64.fullmatch(expected_hash) and actual_hash==expected_hash,
             f"{os_name}: malformed or mismatched native cell hash")
        need(isinstance(cell.get("reference"),str) and cell["reference"].startswith(("fz://blob/","gz://blob/","tz://blob/")),
             f"{os_name}: malformed native cell reference")
    fragments=matrix.get("fragment_rows")
    expected_frag={(w,r,f) for w in ENGINES for r in ENGINES for f in FRAGMENTS}
    need(isinstance(fragments,list) and len(fragments)==len(expected_frag),f"{os_name}: fragment matrix is empty or incomplete")
    need({(c.get("writer"),c.get("reader"),c.get("fragment")) for c in fragments}==expected_frag,
         f"{os_name}: fragment coordinates are incomplete or duplicated")
    need(all(c.get("status")=="pass" for c in fragments),f"{os_name}: fragment check skipped or failed")
    wrong=matrix.get("wrong_store",{})
    need(wrong.get("status")=="pass" and wrong.get("consumer_failed") is True,
         f"{os_name}: wrong-store/corruption safety did not pass")
    concurrent=matrix.get("concurrent",{})
    digest=concurrent.get("expected_hash"); hashes=concurrent.get("hashes")
    need(concurrent.get("status")=="pass",f"{os_name}: concurrency safety did not pass")
    need(isinstance(digest,str) and H64.fullmatch(digest),f"{os_name}: malformed concurrent hash")
    need(isinstance(hashes,dict) and set(hashes)==set(ENGINES) and all(v==digest for v in hashes.values()),
         f"{os_name}: concurrent writer hashes disagree")
    binaries=matrix.get("sibling_shas")
    need(isinstance(binaries,list) and len(binaries)==3 and
         {m.get("engine") for m in binaries if isinstance(m,dict)}==set(ENGINES),
         f"{os_name}: duplicate or missing binary metadata")
    for meta in binaries: validate_binary(meta,os_name,expected)
    return os_name

def claim_evidence_map():
    return [
      {"claim_id":"cross_engine_blob_expand",
       "statement":"Full-hash ZeroRef v1 blob refs expand across FSZero, GraphZero, and TokenZero under a verified shared CAS.",
       "public_surfaces":["README.md","docs/codemode.md","docs/mcp.md","crates/tokenzero-mcp-compat/src/catalog.rs"],
       "evidence":["native_evidence/*/matrix/rows/0/cells","native_evidence/*/matrix/sibling_shas"],
       "capability_fields":{"enabled":True,"shared_cas":True,"blob_ref_expand":True,"cross_engine":True,"portable_ref_kinds":["blob"]}},
      {"claim_id":"byte_and_line_fragments",
       "statement":"Portable blob refs support authenticated #B byte and #L line fragments.",
       "public_surfaces":["docs/codemode.md"],
       "evidence":["native_evidence/*/matrix/fragment_rows"],
       "capability_fields":{"fragment_selectors":["#B","#L"]}},
      {"claim_id":"migration_and_rollback",
       "statement":"Fresh, upgraded, shared, isolated, incompatible, corrupt, disabled, and rollback lifecycle paths are smoke-tested on every release OS.",
       "public_surfaces":["docs/install.md",".github/workflows/ci.yml"],
       "evidence":["lifecycle_evidence/*/cells"],
       "capability_fields":{}},
      {"claim_id":"non_blob_not_portable",
       "statement":"Execution, error, session, file, graph, index, and unit refs are not portable across engines.",
       "public_surfaces":["docs/codemode.md"],
       "evidence":["contract limitation"],
       "capability_fields":{"unsupported_portable_ref_kinds":["execution","error","session","file","graph","index","unit"]}},
      {"claim_id":"performance_deferred",
       "statement":"Correctness evidence does not authorize zero-copy, latency, or performance claims.",
       "public_surfaces":["docs/codemode.md"],
       "evidence":["deferred:tokenzero-9pb","deferred:tokenzero-485"],
       "capability_fields":{}},
    ]

def aggregate(docs,expected,lifecycle_docs):
    need(set(expected)==set(ENGINES) and all(isinstance(v,str) and H40.fullmatch(v) for v in expected.values()),
         "expected commits must be lowercase 40-character SHAs for all engines")
    by_os={}
    for doc in docs:
        os_name=validate(doc,expected)
        need(os_name not in by_os,f"duplicate native evidence for {os_name}")
        by_os[os_name]=doc
    need(set(by_os)==set(OSES),f"required OS evidence mismatch: got {sorted(by_os)}, expected {list(OSES)}")
    lifecycle_by_os={}
    for doc in lifecycle_docs:
        os_name=validate_lifecycle(doc)
        need(os_name not in lifecycle_by_os,f"duplicate lifecycle evidence for {os_name}")
        lifecycle_by_os[os_name]=doc
    need(set(lifecycle_by_os)==set(OSES),f"required lifecycle OS evidence mismatch: got {sorted(lifecycle_by_os)}, expected {list(OSES)}")
    return {"schema":MERGED,"zeroref_version":"v1","status":"green","required_oses":list(OSES),
            "expected_commits":expected,"rows":[by_os[o]["matrix"]["rows"][0] for o in OSES],
            "native_evidence":[by_os[o] for o in OSES],"lifecycle_evidence":[lifecycle_by_os[o] for o in OSES],
            "claim_evidence":claim_evidence_map(),
            "source_evidence_sha256":{o:hashlib.sha256(json.dumps(by_os[o],sort_keys=True).encode()).hexdigest() for o in OSES},
            "lifecycle_evidence_sha256":{o:hashlib.sha256(json.dumps(lifecycle_by_os[o],sort_keys=True).encode()).hexdigest() for o in OSES}}

def fixture(os_name,expected):
    digest="a"*64
    return {"schema":SCHEMA,"zeroref_version":"v1","host_os":os_name,
      "scope":{"explicit":True,"kind":"native-os","os":os_name},"matrix":{"status":"green",
      "rows":[{"os":os_name,"cells":[{"writer":w,"reader":r,"payload":p,"status":"pass","expected_hash":digest,"actual_hash":digest,"reference":"tz://blob/"+digest} for w in ENGINES for r in ENGINES for p in PAYLOADS]}],
      "fragment_rows":[{"writer":w,"reader":r,"fragment":f,"status":"pass"} for w in ENGINES for r in ENGINES for f in FRAGMENTS],
      "wrong_store":{"status":"pass","consumer_failed":True},
      "concurrent":{"status":"pass","expected_hash":digest,"hashes":{e:digest for e in ENGINES}},
      "sibling_shas":[{"engine":e,"path":"/"+e,"sha256":digest,"version":"1.0.0","commit":expected[e],"os":os_name} for e in ENGINES]}}

def reject(docs,expected,lifecycle_docs,text):
    try: aggregate(docs,expected,lifecycle_docs)
    except ValueError as error: need(text in str(error),f"expected {text!r}, got {error!r}")
    else: raise AssertionError(f"negative case passed: {text}")

def lifecycle_fixture(os_name):
    return {"schema":LIFECYCLE_SCHEMA,"os":os_name,"status":"pass",
            "cells":[{"test":name,"status":"pass","os":os_name,"engine":"tokenzero"} for name in LIFECYCLES]}

def self_test():
    expected={"fszero":"1"*40,"graphzero":"2"*40,"tokenzero":"3"*40}
    good=[fixture(o,expected) for o in OSES]
    lifecycle=[lifecycle_fixture(o) for o in OSES]
    merged=aggregate(good,expected,lifecycle)
    need(merged["status"]=="green","representative merge failed")
    need({row["claim_id"] for row in merged["claim_evidence"]}=={"cross_engine_blob_expand","byte_and_line_fragments","migration_and_rollback","non_blob_not_portable","performance_deferred"},"claim/evidence map incomplete")
    cross=next(row for row in merged["claim_evidence"] if row["claim_id"]=="cross_engine_blob_expand")
    need(cross["capability_fields"]["portable_ref_kinds"]==["blob"],"claim capability mapping drifted")
    reject(good[:-1],expected,lifecycle,"required OS evidence mismatch")
    reject(good,expected,lifecycle[:-1],"required lifecycle OS evidence mismatch")
    bad=copy.deepcopy(good); bad[0]["matrix"]["rows"][0]["cells"][0]["status"]="skip"; reject(bad,expected,lifecycle,"skipped or failed")
    bad=copy.deepcopy(good); bad[1]["matrix"]["wrong_store"]["status"]="fail"; reject(bad,expected,lifecycle,"wrong-store")
    bad=copy.deepcopy(good); bad[2]["matrix"]["sibling_shas"][0]["commit"]="unknown"; reject(bad,expected,lifecycle,"unknown")
    bad=copy.deepcopy(good); bad[2]["matrix"]["sibling_shas"][1]["sha256"]="bad"; reject(bad,expected,lifecycle,"malformed binary SHA-256")
    bad_lifecycle=copy.deepcopy(lifecycle); bad_lifecycle[0]["cells"][0]["status"]="skip"; reject(good,expected,bad_lifecycle,"lifecycle cell skipped")
    with tempfile.TemporaryDirectory() as tmp:
        for doc in good: (Path(tmp)/(doc["host_os"]+".json")).write_text(json.dumps(doc))
        need(len(documents(Path(tmp)))==3,"artifact discovery failed")
    print("ZeroRef evidence aggregation self-test: pass")

def main():
    p=argparse.ArgumentParser(); p.add_argument("--input-dir",type=Path); p.add_argument("--output",type=Path)
    p.add_argument("--fszero-commit"); p.add_argument("--graphzero-commit"); p.add_argument("--tokenzero-commit"); p.add_argument("--self-test",action="store_true")
    a=p.parse_args()
    if a.self_test: self_test(); return
    need(a.input_dir is not None and a.output is not None,"--input-dir and --output are required")
    expected={"fszero":a.fszero_commit,"graphzero":a.graphzero_commit,"tokenzero":a.tokenzero_commit}
    merged=aggregate(documents(a.input_dir),expected,lifecycle_documents(a.input_dir))
    a.output.parent.mkdir(parents=True,exist_ok=True); a.output.write_text(json.dumps(merged,indent=2)+"\n")
    print(f"merged green ZeroRef evidence: {a.output}")
if __name__=="__main__":
    try: main()
    except (AssertionError,ValueError) as error: raise SystemExit(f"ZeroRef evidence gate failed: {error}") from error
