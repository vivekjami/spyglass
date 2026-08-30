#!/usr/bin/env python3
"""Phase 9 acceptance: the hardened action path, exercised live.

Run it on a FRESH S1 fault (right after `just scenario s1`, before any agent
touches it): the verification tests want the clean pre-incident baseline the
scenario provides. Through the deployer's MCP surface (the path the harness
uses) and its CLI (the operator's path); leaves payments at v1.

  VERIFY        the real fix (propose -> rollback) and then the engine's
                verdicts: the incident closes only after two consecutive
                clean checks >= 15 s apart -> a `verified_recovery` ledger
                entry; a check inside the interval is paced BY THE ENGINE --
                it waits out the remainder, reports `waited_secs`, and is
                then counted (Phase 11 F4; before that it was refused as
                `too_soon`, which the MCP surface can no longer return)
  ESCALATE      the fault comes back right after the fix and stays: once the
                post window is no better than the incident -> `worsening` ->
                an `escalation` ledger entry, terminal; nothing else is touched
  DOUBLE-FIRE   propose once, rollback twice with the same proposal_id ->
                exactly one rollback (a new D-n) and one recorded no-op
  TOCTOU        propose, then an operator changes routing by hand, then the
                approved rollback arrives -> aborted: version mismatch;
                nothing changes, no D-n is minted
  EXPIRED       a proposal past its expiry is refused -> aborted: expired;
                the world does not move
  RESTATED      a rollback whose restated evidence differs from the minted
                proposal is refused; the faithful restatement then executes
  BUDGET        the 61st tool call inside a minute is refused by the engine

Every refusal is a journal entry with its reason. Exit 0 on PASS, 3 on
FAIL. Raw responses go to data/checks/gate-check.json.

  scripts/gate-check.py
"""
from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mcp_client as m  # noqa: E402
from mcp_client import call, session, wait_ready  # noqa: E402

DEPLOYER = "http://localhost:8792/mcp"
ENGINE = "http://localhost:8791/mcp"


def _verify_interval() -> int:
    """`[verify] interval_secs` from the same config the engine loads."""
    try:
        import tomllib
        with open(Path(__file__).resolve().parent.parent / "spyglass.toml", "rb") as fh:
            return int(tomllib.load(fh)["verify"]["interval_secs"])
    except Exception:
        return 15


INTERVAL = _verify_interval()
CLI = ["./target/release/deployer", "--data-dir", "data/deploy"]
JOURNAL = Path("data/deploy/journal.jsonl")


def cli(*args: str) -> dict:
    p = subprocess.run([*CLI, *args], capture_output=True, text=True)
    line = p.stdout.strip().splitlines()[-1] if p.stdout.strip() else "{}"
    return {"rc": p.returncode, "entry": json.loads(line) if line.startswith("{") else {}, "stderr": p.stderr.strip()}


def journal() -> list[dict]:
    return [json.loads(l) for l in JOURNAL.read_text().splitlines() if l.strip()]


def current(service: str = "payments") -> str:
    return json.loads(Path("data/deploy/current.json").read_text())[service]["version"]


def ensure(version: str) -> None:
    if current() != version:
        cli("deploy", "payments", version, "--actor", "s9-check")
        time.sleep(0.3)


def raw_call(sid: str, name: str, args: dict) -> dict:
    """The deployer's tools return plain JSON (no {result, meta} envelope)."""
    _, raw = m._post({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": args}}, sid, url=DEPLOYER)
    data = [json.loads(l[6:]) for l in raw.splitlines() if l.startswith("data: ") and l[6:].strip()]
    msg = data[-1]
    if "error" in msg:
        raise RuntimeError(f"{name}: {msg['error'].get('message')}")
    res = msg["result"]
    if res.get("isError"):
        raise RuntimeError(res["content"][0]["text"][:400])
    return json.loads(res["content"][0]["text"])


def propose(sid: str, eids: list[str]) -> dict:
    return raw_call(sid, "propose_rollback", {"service": "payments", "to_version": "v1", "justification_eids": eids})


def rollback(sid: str, p: dict, **override) -> dict:
    args = {"proposal_id": p["proposal_id"], "service": p["service"], "to_version": p["to_version"],
            "expected_current": p["expected_current"], "justification_eids": p["justification_eids"]}
    args.update(override)
    return raw_call(sid, "rollback", args)


def verify(sid: str, deploy_id: str) -> dict:
    v = call(sid, "verify_recovery", {"service": "payments", "deploy_id": deploy_id}, url=ENGINE)
    r = v["result"]
    print(f"  check {r['check_n']}{'' if r['counted'] else ' (not counted)'}: {r['status']} (post {r['rates']['post']:.1%} over {r['requests']['post']} req; "
          f"baseline {r['rates']['baseline']:.1%}, tol {r['rates']['tolerance']:.1%}; incident {r['rates']['incident']:.1%}) eid {v['meta']['eids']} → {r['next'][:72]}")
    return v


def ledger_of(sid: str) -> list[dict]:
    lp = Path("ledger") / f"{sid}.jsonl"
    return [json.loads(l) for l in lp.read_text().splitlines() if l.strip()] if lp.exists() else []


def main() -> None:
    fails: list[str] = []
    raw: dict = {}
    dsid = session(url=DEPLOYER, name="gate-check")
    print(f"deployer session {dsid}; payments currently {current()}")
    if current() != "v2":
        print("NOTE: the fault is not active; the verification tests want a fresh `just scenario s1`. Injecting v2 now, but the baseline may be dirty.")
        ensure("v2")
        time.sleep(20)

    # ---- VERIFY: the fix, then the engine's verdicts ------------------------
    esid = session(url=ENGINE, name="gate-check-verify")
    wait_ready(esid, url=ENGINE)
    p = propose(dsid, ["E1", "E2", "E3"])
    fix = rollback(dsid, p)
    action_id = fix["journal_entry"].get("deploy_id")
    print(f"\nVERIFY: proposal {p['proposal_id'][:8]} → rollback {fix['outcome']} {action_id}; payments now {current()}")
    time.sleep(1.5)
    checks = [verify(esid, action_id)]          # right after: insufficient data
    time.sleep(5)
    checks.append(verify(esid, action_id))      # 5 s later: the engine waits out the remainder, then counts it
    for _ in range(4):
        time.sleep(15)
        checks.append(verify(esid, action_id))
        if checks[-1]["result"]["closed"] or checks[-1]["result"]["escalate"]:
            break
    last = checks[-1]["result"]
    entries = ledger_of(esid)
    closing = [e for e in entries if e["tool"] == "verified_recovery"]
    print(f"  ledger: {[e['tool'] for e in entries]}; closing: {closing[0]['summary'] if closing else 'NONE'}")
    raw["verify_close"] = {"proposal": p, "fix": fix, "checks": checks, "ledger": entries}
    if not (last["closed"] and last["consecutive_clean"] >= 2 and len(closing) == 1):
        fails.append(f"VERIFY: not closed: last status {last['status']}, closing entries {len(closing)}")
    c1 = checks[1]["result"]
    since1 = c1.get("since_last_check_secs") or 0
    if not (c1.get("waited_secs", 0) > 0 and c1.get("counted") and since1 >= INTERVAL - 1):
        fails.append(
            f"VERIFY: the engine did not pace a check {INTERVAL // 3} s after the last "
            f"(waited {c1.get('waited_secs')} s, since_last {since1} s, counted {c1.get('counted')}) "
            f"-- it must hold the call open for the rest of the {INTERVAL} s interval, not refuse it"
        )
    if last.get("recovery_changepoint"):
        rc = last["recovery_changepoint"]
        print(f"  recovery changepoint: {rc['series']} down at {rc['at'][11:23]}")
    else:
        print("  (no recovery changepoint landed yet -- reported, not required)")

    # ---- ESCALATE: the fix did not hold -------------------------------------
    # A new investigation; the fault comes back right after the action and
    # stays. Once the 60 s post window is entirely faulty, the post rate is
    # no better than the incident -> `worsening` -> escalate, terminal.
    esid2 = session(url=ENGINE, name="gate-check-escalate")
    cli("deploy", "payments", "v2", "--actor", "s9-check")
    print(f"\nESCALATE: fault re-introduced after the fix ({action_id}); payments now {current()}; waiting for the post window to turn")
    time.sleep(66)
    esc = [verify(esid2, action_id)]
    for _ in range(2):
        if esc[-1]["result"]["escalate"] or esc[-1]["result"]["closed"]:
            break
        time.sleep(15)
        esc.append(verify(esid2, action_id))
    res = esc[-1]["result"]
    entries2 = ledger_of(esid2)
    escal = [e for e in entries2 if e["tool"] == "escalation"]
    print(f"  ledger: {[e['tool'] for e in entries2]}; {escal[0]['summary'] if escal else 'NO escalation entry'}")
    raw["verify_escalate"] = {"checks": esc, "ledger": entries2}
    if not (res["escalate"] and res["status"] in ("worsening", "timeout") and len(escal) == 1):
        fails.append(f"ESCALATE: status {res['status']} escalate {res['escalate']} entries {[e['tool'] for e in entries2]}")
    else:
        after = verify(esid2, action_id)["result"]
        if not (after["status"] == "escalated" and after["escalate"]):
            fails.append(f"ESCALATE: a later check was {after['status']}, expected terminal 'escalated'")

    # ---- DOUBLE-FIRE -------------------------------------------------------
    n0 = len(journal())
    p = propose(dsid, ["E1", "E2", "E7"])
    r1 = rollback(dsid, p)
    r2 = rollback(dsid, p)
    j = journal()[n0:]
    kinds = [e["kind"] for e in j]
    print(f"\nDOUBLE-FIRE: proposal {p['proposal_id'][:8]} (expected_current {p['expected_current']}, expires {p['expires_at'][11:19]}) → "
          f"rollback #1 {r1['outcome']} {r1['journal_entry'].get('deploy_id')} | rollback #2 {r2['outcome']}: {r2['journal_entry'].get('note')}")
    print(f"  journal kinds added: {kinds}; payments now {current()}")
    raw["double_fire"] = {"proposal": p, "r1": r1, "r2": r2, "journal": j}
    if not (r1["outcome"] == "executed" and r2["outcome"] == "noop" and kinds == ["proposal", "rollback", "noop"] and current() == "v1"):
        fails.append(f"DOUBLE-FIRE: outcomes {r1['outcome']}/{r2['outcome']}, journal {kinds}, payments {current()}")
    if r1["journal_entry"].get("request_id") != p["proposal_id"]:
        fails.append("DOUBLE-FIRE: the rollback's request_id is not the minted proposal_id")
    if len([e for e in j if e.get("deploy_id")]) != 1:
        fails.append("DOUBLE-FIRE: more than one deploy id minted")

    # ---- TOCTOU: approve after a manual rollback ---------------------------
    ensure("v2")
    n0 = len(journal())
    p = propose(dsid, ["E1"])
    manual = cli("deploy", "payments", "v1", "--actor", "operator")  # the human fixes it while the gate is pending
    time.sleep(0.2)
    r = rollback(dsid, p)
    j = journal()[n0:]
    print(f"\nTOCTOU: proposal {p['proposal_id'][:8]} expected_current {p['expected_current']}; operator deployed v1 ({manual['entry'].get('deploy_id')}); "
          f"approved rollback → {r['outcome']}: {r['journal_entry'].get('note')}")
    print(f"  journal kinds added: {[e['kind'] for e in j]}; payments now {current()}")
    raw["toctou"] = {"proposal": p, "manual": manual, "r": r, "journal": j}
    if not (r["outcome"] == "aborted" and "version mismatch" in (r["journal_entry"].get("note") or "") and current() == "v1"):
        fails.append(f"TOCTOU: outcome {r['outcome']} note {r['journal_entry'].get('note')}")
    if r["journal_entry"].get("deploy_id"):
        fails.append("TOCTOU: the aborted rollback minted a deploy id")

    # ---- EXPIRED -----------------------------------------------------------
    ensure("v2")
    n0 = len(journal())
    short = cli("propose", "payments", "v1", "--eid", "E1", "--ttl-secs", "1", "--actor", "agent")["entry"]
    time.sleep(2.0)
    pe = {"proposal_id": short["request_id"], "service": short["service"], "to_version": short["version"],
          "expected_current": short["expected_current"], "justification_eids": short["justification_eids"]}
    r = rollback(dsid, pe)
    j = journal()[n0:]
    print(f"\nEXPIRED: proposal {pe['proposal_id'][:8]} ttl 1 s (expires {short['expires_at'][11:23]}) → after 2 s the approved rollback → {r['outcome']}: {r['journal_entry'].get('note')}")
    print(f"  journal kinds added: {[e['kind'] for e in j]}; payments still {current()}")
    raw["expired"] = {"proposal": short, "r": r, "journal": j}
    if not (r["outcome"] == "aborted" and "expired" in (r["journal_entry"].get("note") or "") and current() == "v2"):
        fails.append(f"EXPIRED: outcome {r['outcome']} note {r['journal_entry'].get('note')} payments {current()}")

    # ---- RESTATED ----------------------------------------------------------
    n0 = len(journal())
    p = propose(dsid, ["E1", "E2"])
    bad = rollback(dsid, p, justification_eids=["E9"])
    good = rollback(dsid, p)
    j = journal()[n0:]
    print(f"\nRESTATED: proposal {p['proposal_id'][:8]} eids {p['justification_eids']}; rollback restating [E9] → {bad['outcome']}: {(bad['journal_entry'].get('note') or '')[:90]}")
    print(f"  faithful restatement → {good['outcome']} {good['journal_entry'].get('deploy_id')}; journal kinds added: {[e['kind'] for e in j]}; payments now {current()}")
    raw["restated"] = {"proposal": p, "bad": bad, "good": good, "journal": j}
    if not (bad["outcome"] == "aborted" and "differs" in (bad["journal_entry"].get("note") or "") and good["outcome"] == "executed" and current() == "v1"):
        fails.append(f"RESTATED: bad {bad['outcome']} good {good['outcome']} payments {current()}")

    # ---- BUDGET ------------------------------------------------------------
    bsid = session(url=ENGINE, name="gate-check-budget")
    refused = None
    for i in range(1, 70):
        try:
            call(bsid, "freshness_watermark", {}, url=ENGINE)
        except Exception as e:
            refused = (i, str(e))
            break
    print(f"\nBUDGET: call #{refused[0] if refused else '-'} refused: {refused[1][:110] if refused else 'never'}")
    raw["budget"] = refused
    if not refused or refused[0] != 61:
        fails.append(f"BUDGET: expected the 61st call to be refused, got {refused}")

    Path("data/checks").mkdir(parents=True, exist_ok=True)
    Path("data/checks/gate-check.json").write_text(json.dumps(raw, indent=1, default=str))
    print()
    if fails:
        print("FAIL")
        for x in fails:
            print(f"  - {x}")
        sys.exit(3)
    print("PASS: engine closes after 2 clean checks >= 15 s apart (verified_recovery) and escalates on worsening (escalation, terminal); "
          "double-fire = 1 rollback + 1 noop; manual change → aborted (version mismatch); expired → aborted; restated mismatch → aborted; 61st call refused")


if __name__ == "__main__":
    main()
