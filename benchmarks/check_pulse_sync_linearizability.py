#!/usr/bin/env python3
"""Bounded linearizability checker for tokenzero-pulse record/sync histories."""
import itertools
import json
from pathlib import Path

def precedence(history):
    return sorted((a["id"], b["id"]) for a in history for b in history
                  if a["id"] != b["id"] and a["end"] < b["start"])

def legal(order, edges):
    pos = {op["id"]: i for i, op in enumerate(order)}
    return all(pos[a] < pos[b] for a, b in edges)

def sequentially_valid(order):
    ledger = []
    for op in order:
        if op["kind"] == "record":
            ledger.append(op["event"])
        elif op["kind"] == "sync":
            if op["result"] != ledger:
                return False
        else:
            raise ValueError(op["kind"])
    return True

def check(case):
    history = case["history"]
    edges = precedence(history)
    examined = 0
    witness = None
    for order in itertools.permutations(history):
        if not legal(order, edges):
            continue
        examined += 1
        if sequentially_valid(order):
            witness = [op["id"] for op in order]
            break
    return {"name": case["name"], "expected": case["expected"],
            "linearizable": witness is not None, "real_time_edges": edges,
            "legal_orders_examined": examined, "witness": witness}

CASES = [
 {"name":"overlap_serializes_at_lock","expected":True,"history":[
  {"id":"record-A","kind":"record","event":"A","start":0,"end":4},
  {"id":"sync-AB","kind":"sync","result":["A","B"],"start":1,"end":8},
  {"id":"record-B","kind":"record","event":"B","start":3,"end":6}]},
 {"name":"future_event_included_before_invocation","expected":False,"history":[
  {"id":"sync-future","kind":"sync","result":["L"],"start":0,"end":3},
  {"id":"record-late","kind":"record","event":"L","start":4,"end":7}]},
 {"name":"completed_event_missing_from_later_sync","expected":False,"history":[
  {"id":"record-done","kind":"record","event":"D","start":0,"end":2},
  {"id":"sync-stale","kind":"sync","result":[],"start":3,"end":6}]}]

def main():
    results = [check(case) for case in CASES]
    passed = all(r["linearizable"] == r["expected"] for r in results)
    output = {"contract":"record appends one event; sync returns exact ordered ledger snapshot",
              "real_time_rule":"response(A) < invocation(B) implies A precedes B",
              "cases":results, "passed":passed}
    print(json.dumps(output, indent=2, sort_keys=True))
    raise SystemExit(0 if passed else 1)
if __name__ == "__main__":
    main()
