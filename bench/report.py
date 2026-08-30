#!/usr/bin/env python3
"""Aggregate bench/results/*.json into the benchmark tables (Phase 10).

Reads every committed run file, scores each against its scenario's
pre-registered ground truth (scenarios/*/ground-truth.yaml -- the `match`
and `scoring` blocks), and rewrites the generated regions of
docs/benchmark.md and README.md. Tables are never hand-edited: every number
here traces to a run file, and every run file is committed, failures
included.

Scoring (scenarios/SCHEMA.md, "Scoring semantics"):
  success      the terminal state equals correct_action (rollback of the right
               service/version executed exactly once; or nothing executed and
               the verdict's action is the right report/refusal)
  no wrong action  no rollback executed other than the correct one (S3/S6: none
               at all) -- the safety floor, shown next to success because a
               right report filed under the wrong exit is not a wrong action
  rca          the closing `verdict` block's culprit_service and culprit_change
               are both accepted by scoring.verdict (no block -> not correct)
  precision    relevant cited evidence ids / cited ids (Spyglass conditions --
               the baseline has no evidence ids; that is a finding, not a gap)
  recall       key expected-evidence entries with a relevant citation / key entries
  verified     rollback scenarios: the engine's verified_recovery entry
               (Spyglass), or the agent re-checked a metric after acting and
               the runner's post-run edge 5xx is under pre_fault_max (baseline)
  t_hyp        alert -> first assistant message containing every term of any
               scoring.first_hypothesis_terms group
  decoy_ment   interim assistant messages mentioning a scoring.decoy_terms term

Only runs with "benchmark": true (bench/run.py) enter the tables; earlier
per-phase runs stay in the "preliminary" section of docs/benchmark.md.

  bench/report.py            # rewrite the tables
  bench/report.py --print    # tables to stdout only
  bench/report.py --all      # include non-benchmark runs too (development)
"""
from __future__ import annotations

import argparse
import glob
import json
import re
import statistics
import sys
from datetime import datetime, timezone
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "bench/results"
PRICES = ROOT / "bench/price-sheet.json"
COND_ORDER = ["baseline", "spyglass", "ablation-no-novelty"]
COND_LABEL = {"baseline": "BASELINE (raw tools)", "spyglass": "SPYGLASS", "ablation-no-novelty": "ABLATION A1 (no novelty)"}
SCN_ORDER = ["s1", "s2", "s3", "s6", "s4", "s5"]


# ---------------------------------------------------------------- loading
def ts(s: str | None) -> datetime | None:
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except ValueError:
        return None


def ground_truths() -> dict[str, dict]:
    out = {}
    for p in sorted(glob.glob(str(ROOT / "scenarios/*/ground-truth.yaml"))):
        gt = yaml.safe_load(Path(p).read_text())
        out[gt["scenario"].split("-")[0]] = gt
    return out


def load_runs(include_all: bool) -> list[dict]:
    runs = []
    for p in sorted(RESULTS.glob("*.json")):
        try:
            r = json.loads(p.read_text())
        except json.JSONDecodeError:
            continue
        if not include_all and not r.get("benchmark"):
            continue
        r["_file"] = p.name
        if "valid" not in r:  # runs from before Phase 10 carry no validity flag
            r["valid"] = r.get("outcome") == "completed"
            r["invalid_reason"] = None if r["valid"] else r.get("outcome")
        runs.append(r)
    return runs


# ---------------------------------------------------------------- evidence join
def parse_json(s):
    if isinstance(s, (dict, list)):
        return s
    try:
        return json.loads(s)
    except (TypeError, json.JSONDecodeError):
        return None


def eid_items(run: dict) -> dict[str, dict]:
    """eid -> {item, tool, response_result} from every tool response in the trace."""
    calls = {}
    for e in run.get("events", []):
        if e.get("type") == "model.message":
            for tc in e.get("tool_calls") or []:
                calls[tc.get("id")] = tc["function"]["name"]
    out = {}
    for e in run.get("events", []):
        if e.get("type") != "tool.response":
            continue
        body = parse_json(e.get("content"))
        if not isinstance(body, dict):
            continue
        res = body.get("result")
        tool = calls.get(e.get("tool_call_id"), "?")
        if not isinstance(res, dict):
            continue
        items = res.get("items")
        if isinstance(items, list):
            for it in items:
                if isinstance(it, dict) and it.get("eid"):
                    out.setdefault(it["eid"], {"item": it, "tool": tool, "result": res})
    return out


def labels_of(series: str) -> dict[str, str]:
    return dict(re.findall(r'(\w+)="([^"]*)"', series or ""))


def item_services(it: dict) -> set[str]:
    s = set()
    for k in ("service", "instance"):
        if isinstance(it.get(k), str):
            s.add(it[k])
    for k in ("services", "instances"):
        for x in it.get(k) or []:
            if isinstance(x, str):
                s.add(x)
    lab = labels_of(it.get("series", ""))
    s.update(v for k, v in lab.items() if k in ("service", "instance"))
    return s


def matches(it: dict, ctx: dict, m: dict, t_fault: datetime | None) -> bool:
    if "kind" in m and it.get("kind") not in m["kind"]:
        return False
    pat = str(it.get("pattern", ""))
    if "pattern_contains" in m and m["pattern_contains"] not in pat:
        return False
    if "pattern_contains_any" in m and not any(x in pat for x in m["pattern_contains_any"]):
        return False
    if "metric_in" in m and str(it.get("series", "")).split("{")[0] not in m["metric_in"]:
        return False
    if "service" in m:
        svc = m["service"]
        if svc not in item_services(it) and not any(x.startswith(svc + "-") for x in item_services(it)):
            return False
    if "direction" in m and it.get("direction") != m["direction"]:
        return False
    if "deploy_id" in m and it.get("deploy_id") != m["deploy_id"]:
        return False
    if "nearest_deploy_id" in m and ((it.get("nearest_deploy") or {}).get("deploy_id") != m["nearest_deploy_id"]):
        return False
    if "at_after_fault_secs" in m and t_fault is not None:  # runs without an embedded manifest skip the time test
        at = ts(it.get("at") or it.get("first_seen") or it.get("ts"))
        if not at:
            return False
        lo, hi = m["at_after_fault_secs"]
        if not (lo <= (at - t_fault).total_seconds() <= hi):
            return False
    if "replay_verdict" in m and ((ctx.get("result") or {}).get("comparison") or {}).get("verdict") != m["replay_verdict"]:
        return False
    if "origin_5xx_instance" in m:
        origin = str((it.get("outcome") or {}).get("origin_5xx", ""))
        if m["origin_5xx_instance"] not in origin:
            return False
    return True


def classify(run: dict, gt: dict) -> dict:
    """Join the RCA's cited eids to the ground truth."""
    items = eid_items(run)
    cited = (run.get("ledger") or {}).get("eids_cited_valid") or []
    t_fault = ts(((run.get("scenario_run") or {}).get("t_fault")))
    expected = [e for e in gt["expected_evidence"] if e.get("scored", True) and isinstance(e.get("match"), dict)]
    decoys = [d for d in gt["decoys"] if isinstance(d.get("match"), dict)]
    hit = {i: False for i in range(len(expected))}
    relevant, decoy_cites, other = [], [], []
    per = {}
    for eid in cited:
        ctx = items.get(eid)
        it = (ctx or {}).get("item")
        if not it:
            other.append(eid)
            per[eid] = "unresolved"
            continue
        rel = False
        for i, ex in enumerate(expected):
            if matches(it, ctx, ex["match"], t_fault):
                hit[i] = True
                rel = True
        if rel:
            relevant.append(eid)
            per[eid] = "relevant"
            continue
        dec = next((d for d in decoys if matches(it, ctx, d["match"], t_fault)), None)
        if dec and not dec.get("relevant"):
            decoy_cites.append(eid)
            per[eid] = f"decoy:{dec['kind']}"
        elif dec:
            relevant.append(eid)
            per[eid] = f"symptom:{dec['kind']}"
        else:
            other.append(eid)
            per[eid] = f"other:{it.get('kind')}"
    key_idx = [i for i, ex in enumerate(expected) if ex.get("key")]
    return {
        "cited": len(cited),
        "relevant": len(relevant),
        "decoy_citations": len(decoy_cites),
        "other": len(other),
        "precision": (len(relevant) / len(cited)) if cited else None,
        "recall": (sum(hit[i] for i in key_idx) / len(key_idx)) if key_idx else None,
        "key_hit": [expected[i]["kind"] + ("" if hit[i] else " MISSING") for i in key_idx],
        "per_eid": per,
    }


# ---------------------------------------------------------------- verdict + trace
VERDICT_RE = re.compile(r"```verdict\s*\n(.*?)```", re.S | re.I)


def parse_verdict(text: str) -> dict | None:
    m = VERDICT_RE.search(text or "")
    if not m:
        return None
    v = {}
    for line in m.group(1).splitlines():
        if ":" in line:
            k, val = line.split(":", 1)
            v[k.strip().lower()] = val.strip().strip("`*").strip()
    norm = lambda x: "none" if str(x).strip().lower() in ("", "none", "null", "n/a", "-", "no change") else str(x).strip()
    return {
        "culprit_service": norm(v.get("culprit_service", "")).lower(),
        "culprit_change": norm(v.get("culprit_change", "")),
        "cause": v.get("cause", ""),
        "action": v.get("action", "").lower().replace("-", "_").replace(" ", "_"),
        "evidence_label": v.get("evidence_label", "").lower(),
    }


def assistant_messages(run: dict) -> list[tuple[datetime | None, str]]:
    out = []
    for e in run.get("events", []):
        if e.get("type") == "model.message" and e.get("content") and isinstance(e["content"], str) and e.get("thread_id", "main") == "main":
            out.append((ts(e.get("created_at")), e["content"]))
    return out


def t_first_hypothesis(run: dict, gt: dict) -> float | None:
    groups = gt.get("scoring", {}).get("first_hypothesis_terms") or []
    t0 = ts(run.get("started_at"))
    for at, text in assistant_messages(run):
        low = text.lower()
        if any(all(term.lower() in low for term in g) for g in groups if g):
            return round((at - t0).total_seconds(), 1) if at and t0 else None
    return None


def decoy_mentions(run: dict, gt: dict) -> int:
    terms = [t.lower() for t in gt.get("scoring", {}).get("decoy_terms") or []]
    msgs = assistant_messages(run)[:-1]  # interim only
    return sum(1 for _, text in msgs if any(t in text.lower() for t in terms))


def baseline_rechecked(run: dict) -> bool:
    """Did the agent read a metric after the rollback tool call?"""
    seen_rollback = False
    for e in run.get("events", []):
        if e.get("type") != "model.message":
            continue
        for tc in e.get("tool_calls") or []:
            n = tc["function"]["name"]
            if n == "rollback":
                seen_rollback = True
            elif seen_rollback and n in ("get_metric", "verify_recovery", "tail_logs", "grep_logs"):
                return True
    return False


# ---------------------------------------------------------------- scoring
def cost_usd(run: dict, prices: dict) -> float | None:
    p = prices.get(run.get("model") or "")
    if not p or any(p.get(k) is None for k in ("input_per_1m", "cache_read_per_1m", "output_per_1m")):
        return None
    m = run["metrics"]
    cache = m.get("cache_read_tokens", 0) or 0
    return ((m["input_tokens"] - cache) * p["input_per_1m"] + cache * p["cache_read_per_1m"] + m["output_tokens"] * p["output_per_1m"]) / 1e6


def score(run: dict, gt: dict, prices: dict) -> dict:
    m = run["metrics"]
    correct = gt["correct_action"]
    sv = gt["scoring"]["verdict"]
    verdict = parse_verdict(run.get("final_output", ""))
    rbs = m.get("rollbacks_executed") or []
    right_rb = [r for r in rbs if r.get("service") == correct.get("service") and r.get("to") == correct.get("to_version")]
    if correct["type"] == "rollback":
        success = len(rbs) == 1 and len(right_rb) == 1
        terminal = f"rollback {rbs[0]['service']}->{rbs[0]['to']}" if rbs else (f"proposal only" if m.get("journal_kinds_added", {}).get("proposal") else "no action")
    else:
        success = not rbs and verdict is not None and verdict["action"] == correct["type"]
        terminal = f"WRONG ACTION rollback {rbs[0]['service']}->{rbs[0]['to']}" if rbs else (verdict["action"] if verdict else "no verdict")
    no_wrong_action = not [r for r in rbs if r not in right_rb]
    rca = bool(verdict) and verdict["culprit_service"] in [str(x).lower() for x in sv["culprit_service"]] \
        and verdict["culprit_change"] in [str(x) for x in sv["culprit_change"]]
    ev = classify(run, gt) if run.get("ledger") else None
    V = run.get("verification") or {}
    post = (m.get("error_rate_post_run") or {}).get("rate")
    pre_max = gt["expected_error_rate"]["pre_fault_max"]
    if correct["type"] == "rollback":
        if run.get("ledger"):
            verified = bool(V.get("closed"))
            verified_how = "engine: verified_recovery" if verified else ("engine: escalation" if V.get("escalated") else "engine: not closed")
        else:
            rechecked = baseline_rechecked(run)
            verified = rechecked and post is not None and post <= pre_max
            verified_how = ("agent re-checked; " if rechecked else "no re-check; ") + (f"post-run 5xx {post:.1%}" if post is not None else "no post traffic")
    else:
        verified, verified_how = None, "n/a"
    closed_at = None
    for en in (run.get("ledger") or {}).get("ledger_entries") or []:
        if en.get("tool") == "verified_recovery":
            closed_at = ts(en.get("ts"))
    t0 = ts(run.get("started_at"))
    return {
        "success": success, "terminal": terminal, "no_wrong_action": no_wrong_action,
        "rca": rca, "verdict": verdict,
        "evidence": ev,
        "verified": verified, "verified_how": verified_how,
        "t_rca_secs": m.get("wall_time_secs"),
        "t_close_secs": round((closed_at - t0).total_seconds(), 1) if closed_at and t0 else None,
        "t_hyp_secs": t_first_hypothesis(run, gt),
        "decoy_mentions": decoy_mentions(run, gt),
        "tool_calls": m.get("tool_calls"), "model_calls": m.get("model_calls"),
        "input": m.get("input_tokens"), "cache": m.get("cache_read_tokens", 0) or 0,
        "uncached": (m.get("input_tokens") or 0) - (m.get("cache_read_tokens", 0) or 0),
        "output": m.get("output_tokens"), "total": m.get("total_tokens"),
        "tool_bytes": m.get("tool_response_bytes"),
        "cost": cost_usd(run, prices),
        "post_rate": post,
        "eids_cited": len((run.get("ledger") or {}).get("eids_cited_valid") or []),
        "eids_issued": (run.get("ledger") or {}).get("eids_issued"),
        "recheck": ((run.get("ledger") or {}).get("recheck") or {}).get("verdict"),
        "exec_nonsleep": [c for c in (m.get("sandbox_exec_commands") or []) if c and "sleep" not in c],
    }


# ---------------------------------------------------------------- tables
def fmt_range(vals: list, pct=False, k=False) -> str:
    v = [x for x in vals if x is not None]
    if not v:
        return "—"
    if pct:
        f = lambda x: f"{100*x:.0f}%"
    elif k:
        f = lambda x: f"{x/1000:.0f}k"
    else:
        f = lambda x: f"{x:.0f}" if isinstance(x, (int, float)) and abs(x) >= 10 else (f"{x:.1f}" if isinstance(x, float) else str(x))
    if len(v) == 1:
        return f(v[0])
    mean = statistics.mean(v)
    return f"{f(mean)} [{f(min(v))}..{f(max(v))}]"


def fmt_frac(flags: list) -> str:
    v = [x for x in flags if x is not None]
    return f"{sum(bool(x) for x in v)}/{len(v)}" if v else "—"


def cell_rows(scored: list[dict]) -> list[str]:
    """Headline table: one row per scenario x condition, per-run values as mean [min..max]."""
    rows = ["| Scenario | Condition | n | Success | No wrong action | RCA correct | Evidence P / R | Tool calls | Input tokens (uncached) | Output tokens | Cost | Alert→RCA s | Verified | 1st hypothesis s | Decoy mentions |",
            "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    keys = sorted({(r["scenario"], r["condition"]) for r in scored},
                  key=lambda x: (SCN_ORDER.index(x[0]) if x[0] in SCN_ORDER else 9, COND_ORDER.index(x[1]) if x[1] in COND_ORDER else 9))
    for s, c in keys:
        rs = [r for r in scored if r["scenario"] == s and r["condition"] == c and r.get("valid")]
        inv = [r for r in scored if r["scenario"] == s and r["condition"] == c and not r.get("valid")]
        if not rs:
            rows.append(f"| {s.upper()} | {COND_LABEL.get(c, c)} | 0 | — | — | — | — | — | — | — | — | — | — | — | — |" + (f" {len(inv)} invalid" if inv else ""))
            continue
        S = [r["score"] for r in rs]
        ev = [x["evidence"] for x in S if x["evidence"]]
        pr = f"{fmt_range([e['precision'] for e in ev], pct=True)} / {fmt_range([e['recall'] for e in ev], pct=True)}" if ev else "n/a (no eids)"
        cost = fmt_range([x["cost"] for x in S]) if any(x["cost"] is not None for x in S) else "n/a"
        if cost != "n/a":
            cost = "$" + cost
        ver = fmt_frac([x["verified"] for x in S]) if any(x["verified"] is not None for x in S) else "n/a"
        rows.append(f"| {s.upper()} | {COND_LABEL.get(c, c)} | {len(rs)}{'+' + str(len(inv)) + ' invalid' if inv else ''} | {fmt_frac([x['success'] for x in S])} | {fmt_frac([x['no_wrong_action'] for x in S])} | {fmt_frac([x['rca'] for x in S])} | {pr} | "
                    f"{fmt_range([x['tool_calls'] for x in S])} | {fmt_range([x['input'] for x in S], k=True)} ({fmt_range([x['uncached'] for x in S], k=True)}) | "
                    f"{fmt_range([x['output'] for x in S], k=True)} | {cost} | {fmt_range([x['t_rca_secs'] for x in S])} | {ver} | "
                    f"{fmt_range([x['t_hyp_secs'] for x in S])} | {fmt_range([x['decoy_mentions'] for x in S])} |")
    return rows


def run_rows(scored: list[dict]) -> list[str]:
    rows = ["| Run file | Scenario | Condition | Repeat | Valid | Terminal state | RCA verdict (service / change / action / label) | Evidence cited (relevant / decoy / other) | Recall misses | Calls | Input (cache) | Output | Alert→RCA | Verified | Post-run edge 5xx | Ledger re-check |",
            "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in sorted(scored, key=lambda r: (SCN_ORDER.index(r["scenario"]) if r["scenario"] in SCN_ORDER else 9, COND_ORDER.index(r["condition"]) if r["condition"] in COND_ORDER else 9, r["run_id"])):
        x = r["score"]
        v = x["verdict"]
        vs = f"{v['culprit_service']} / {v['culprit_change']} / {v['action']} / {v['evidence_label']}" if v else "**no verdict block**"
        ev = x["evidence"]
        evs = f"{ev['cited']} ({ev['relevant']} / {ev['decoy_citations']} / {ev['other']})" if ev else "n/a"
        miss = ", ".join(k.replace(" MISSING", "") for k in (ev or {}).get("key_hit", []) if "MISSING" in k) or ("—" if ev else "n/a")
        post = f"{x['post_rate']:.1%}" if x["post_rate"] is not None else "—"
        rows.append(f"| `{r['_file']}` | {r['scenario'].upper()} | {r['condition']} | {r.get('tag','')} | {'yes' if r.get('valid') else '**no** (' + str(r.get('invalid_reason')) + ')'} | "
                    f"{'✅' if x['success'] else '❌'} {x['terminal']} | {'✅' if x['rca'] else '❌'} {vs} | {evs} | {miss} | {x['tool_calls']} | "
                    f"{(x['input'] or 0):,} ({x['cache']:,}) | {(x['output'] or 0):,} | {x['t_rca_secs']} s | {x['verified_how']} | {post} | {x['recheck'] or 'n/a'} |")
    return rows


def readme_rows(scored: list[dict]) -> list[str]:
    rows = ["| Scenario | Condition | Success | No wrong action | RCA acc. | Tool calls | Total tokens | Cost | Latency (alert→RCA) |", "|---|---|---|---|---|---|---|---|---|"]
    keys = sorted({(r["scenario"], r["condition"]) for r in scored},
                  key=lambda x: (SCN_ORDER.index(x[0]) if x[0] in SCN_ORDER else 9, COND_ORDER.index(x[1]) if x[1] in COND_ORDER else 9))
    for s, c in keys:
        rs = [r for r in scored if r["scenario"] == s and r["condition"] == c and r.get("valid")]
        if not rs:
            continue
        S = [r["score"] for r in rs]
        cost = ("$" + fmt_range([x["cost"] for x in S])) if any(x["cost"] is not None for x in S) else "n/a (price sheet empty)"
        rows.append(f"| {s.upper()} | {c} | {fmt_frac([x['success'] for x in S])} | {fmt_frac([x['no_wrong_action'] for x in S])} | {fmt_frac([x['rca'] for x in S])} | {fmt_range([x['tool_calls'] for x in S])} | "
                    f"{fmt_range([x['total'] for x in S], k=True)} | {cost} | {fmt_range([x['t_rca_secs'] for x in S])} s |")
    return rows


def replace_region(path: Path, begin: str, end: str, body: str) -> bool:
    s = path.read_text()
    if begin not in s or end not in s:
        print(f"  markers {begin} / {end} not found in {path}; skipped", file=sys.stderr)
        return False
    pre, rest = s.split(begin, 1)
    _, post = rest.split(end, 1)
    path.write_text(pre + begin + "\n" + body.rstrip() + "\n" + end + post)
    return True


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--print", action="store_true")
    ap.add_argument("--all", action="store_true", help="include non-benchmark runs")
    ap.add_argument("--json", help="also dump the scored runs to this path")
    a = ap.parse_args()
    gts = ground_truths()
    prices = json.loads(PRICES.read_text()) if PRICES.exists() else {}
    runs = load_runs(a.all)
    scored = []
    for r in runs:
        gt = gts.get(r["scenario"])
        if not gt:
            continue
        try:
            r["score"] = score(r, gt, prices)
        except Exception as e:  # a malformed run must not zero the table silently
            print(f"  {r['_file']}: scoring error {e!r}", file=sys.stderr)
            continue
        scored.append(r)
    n_valid = sum(1 for r in scored if r.get("valid"))
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    head = [f"*Generated by `bench/report.py` on {stamp} from {len(scored)} run files ({n_valid} valid) in `bench/results/`; per-cell values are mean [min..max] over valid runs. "
            f"Cost uses `bench/price-sheet.json`{' (empty: tokens are the cost proxy)' if not any(r['score']['cost'] is not None for r in scored) else ''}. Do not edit by hand.*", ""]
    cells = head + cell_rows(scored)
    per_run = ["", f"### Every run (n = {len(scored)}, invalid runs included)", ""] + run_rows(scored)
    doc = "\n".join(cells + per_run)
    readme = "\n".join([f"*{len(scored)} runs ({n_valid} valid), generated by `bench/report.py`; full tables with per-run values in [`docs/benchmark.md`](docs/benchmark.md).*", ""] + readme_rows(scored))
    if a.json:
        Path(a.json).write_text(json.dumps([{k: v for k, v in r.items() if k not in ("events", "turns", "final_output", "ledger")} for r in scored], indent=1, default=str))
    if a.print:
        print(doc)
        return
    ok1 = replace_region(ROOT / "docs/benchmark.md", "<!-- report:begin -->", "<!-- report:end -->", doc)
    ok2 = replace_region(ROOT / "README.md", "<!-- bench-results:begin -->", "<!-- bench-results:end -->", readme)
    print("\n".join(cells))
    print(f"\nwrote docs/benchmark.md={'ok' if ok1 else 'SKIPPED'} README.md={'ok' if ok2 else 'SKIPPED'} ({len(scored)} runs, {n_valid} valid)")
    bad = [r for r in scored if r["score"]["exec_nonsleep"]]
    for r in bad:
        print(f"  NOTE {r['_file']}: sandbox exec beyond sleep: {r['score']['exec_nonsleep']}")


if __name__ == "__main__":
    main()
