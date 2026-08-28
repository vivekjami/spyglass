#!/usr/bin/env python3
"""Validate scenario ground-truth files against scenarios/SCHEMA.md.

Deliberately stdlib + PyYAML only. Fails loudly on the first bad file so a
malformed ground truth cannot silently zero a benchmark column.
"""
import sys
from pathlib import Path

import yaml

EVIDENCE_KINDS = {"novel_template", "changepoint", "deploy", "deploy_correlation", "metric_shift", "absence"}
ACTIONS = {"rollback", "report_only", "refuse_escalate"}
REQUIRED = {"scenario": str, "version": int, "description": str, "seed": int, "timeline": dict,
            "culprit": dict, "expected_evidence": list, "decoys": list, "correct_action": dict,
            "expected_error_rate": dict, "verification_signal": dict}


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
    if "fault" not in tl or not {"service", "version", "deploy_id"} <= set(tl["fault"]):
        errs.append("timeline.fault needs service, version, deploy_id")
    c = gt["culprit"]
    if "service" not in c or "change" not in c or "deploy_id" not in c["change"]:
        errs.append("culprit needs service and change.deploy_id")
    elif c["change"]["deploy_id"] != tl["fault"]["deploy_id"]:
        errs.append("culprit.change.deploy_id must match timeline.fault.deploy_id")
    for i, ev in enumerate(gt["expected_evidence"]):
        if ev.get("kind") not in EVIDENCE_KINDS:
            errs.append(f"expected_evidence[{i}].kind '{ev.get('kind')}' not in {sorted(EVIDENCE_KINDS)}")
    for i, d in enumerate(gt["decoys"]):
        if "kind" not in d or "note" not in d:
            errs.append(f"decoys[{i}] needs kind and note")
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
