#!/usr/bin/env python3
"""Validate scenario ground-truth files against scenarios/SCHEMA.md.

Deliberately stdlib + PyYAML only. Fails loudly on the first bad file so a
malformed ground truth cannot silently zero a benchmark column.
"""
import sys
from pathlib import Path

import yaml

EVIDENCE_KINDS = {"novel_template", "burst_template", "changepoint", "deploy", "deploy_correlation", "metric_shift",
                  "replay_separation", "exemplar", "verification", "absence"}
ACTIONS = {"rollback", "report_only", "refuse_escalate"}
FAULT_KINDS = {"deploy", "redis_fill", "dependency_degradation"}
MATCH_KEYS = {"kind", "pattern_contains", "pattern_contains_any", "metric_in", "service", "direction", "deploy_id",
              "nearest_deploy_id", "at_after_fault_secs", "replay_verdict", "origin_5xx_instance"}
REQUIRED = {"scenario": str, "version": int, "description": str, "seed": int, "alert": str, "timeline": dict,
            "culprit": dict, "expected_evidence": list, "decoys": list, "correct_action": dict,
            "expected_error_rate": dict, "verification_signal": dict, "scoring": dict}


def check(path: Path) -> list[str]:
    gt = yaml.safe_load(path.read_text())
    errs = []
    for k, t in REQUIRED.items():
        if k not in gt:
            errs.append(f"missing key: {k}")
        elif not isinstance(gt[k], t):
            errs.append(f"{k}: expected {t.__name__}, got {type(gt[k]).__name__}")
    if errs:
        return errs
    if gt["scenario"] != path.parent.name:
        errs.append(f"scenario '{gt['scenario']}' != directory '{path.parent.name}'")
    tl = gt["timeline"]
    fault = tl.get("fault") or {}
    fk = fault.get("kind", "deploy")
    if fk not in FAULT_KINDS:
        errs.append(f"timeline.fault.kind '{fk}' not in {sorted(FAULT_KINDS)}")
    if fk == "deploy" and not {"service", "version", "deploy_id"} <= set(fault):
        errs.append("timeline.fault (deploy) needs service, version, deploy_id")
    c = gt["culprit"]
    if "service" not in c or "change" not in c:
        errs.append("culprit needs service and change (a map, or null when the culprit is not a change)")
    elif fk == "deploy":
        if not isinstance(c["change"], dict) or "deploy_id" not in c["change"]:
            errs.append("culprit.change needs deploy_id for a deploy-shaped fault")
        elif c["change"]["deploy_id"] != fault["deploy_id"]:
            errs.append("culprit.change.deploy_id must match timeline.fault.deploy_id")
    elif c["change"] is not None:
        errs.append("culprit.change must be null when the fault is not a change event")
    key_items = 0
    for i, ev in enumerate(gt["expected_evidence"]):
        if ev.get("kind") not in EVIDENCE_KINDS:
            errs.append(f"expected_evidence[{i}].kind '{ev.get('kind')}' not in {sorted(EVIDENCE_KINDS)}")
        if ev.get("scored", True) and not isinstance(ev.get("match"), dict):
            errs.append(f"expected_evidence[{i}] needs a match map (or scored: false)")
        if isinstance(ev.get("match"), dict) and not set(ev["match"]) <= MATCH_KEYS:
            errs.append(f"expected_evidence[{i}].match has unknown keys {sorted(set(ev['match']) - MATCH_KEYS)}")
        key_items += bool(ev.get("key")) and ev.get("scored", True)
    if key_items == 0:
        errs.append("no scored expected_evidence item is key: true (recall would be undefined)")
    for i, d in enumerate(gt["decoys"]):
        if "kind" not in d or "note" not in d:
            errs.append(f"decoys[{i}] needs kind and note")
        if isinstance(d.get("match"), dict) and not set(d["match"]) <= MATCH_KEYS:
            errs.append(f"decoys[{i}].match has unknown keys {sorted(set(d['match']) - MATCH_KEYS)}")
    sc = gt["scoring"]
    v = sc.get("verdict") or {}
    if not (isinstance(v.get("culprit_service"), list) and isinstance(v.get("culprit_change"), list) and v.get("action") in ACTIONS):
        errs.append("scoring.verdict needs culprit_service (list), culprit_change (list), action")
    elif v["action"] != gt["correct_action"].get("type"):
        errs.append("scoring.verdict.action must equal correct_action.type")
    if not isinstance(sc.get("first_hypothesis_terms"), list) or not isinstance(sc.get("decoy_terms"), list):
        errs.append("scoring needs first_hypothesis_terms and decoy_terms (lists)")
    a = gt["correct_action"]
    if a.get("type") not in ACTIONS:
        errs.append(f"correct_action.type '{a.get('type')}' not in {sorted(ACTIONS)}")
    elif a["type"] == "rollback" and not {"service", "to_version"} <= set(a):
        errs.append("correct_action rollback needs service and to_version")
    er = gt["expected_error_rate"]
    pf = er.get("post_fault", {})
    if not (0 <= er.get("pre_fault_max", -1) <= 1 and 0 <= pf.get("min", -1) <= pf.get("max", -1) <= 1):
        errs.append("expected_error_rate needs pre_fault_max and post_fault.{min,max} in [0,1], min<=max")
    if not {"metric", "recovered_when"} <= set(gt["verification_signal"]):
        errs.append("verification_signal needs metric and recovered_when")
    return errs


def main(paths):
    bad = 0
    for p in map(Path, paths):
        errs = check(p)
        print(f"{'FAIL' if errs else 'ok  '} {p}")
        for e in errs:
            print(f"      - {e}")
        bad += bool(errs)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main(sys.argv[1:] or sorted(Path("scenarios").glob("*/ground-truth.yaml")))
