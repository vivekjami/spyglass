//! Post-action verification (README C11, Phase 9): `verify_recovery`.
//!
//! Remediation success is never assumed, and it is never the model's call.
//! After the deployer confirms an action, the agent asks the engine -- every
//! `verify.interval_secs` -- whether the world recovered. The engine judges
//! from request outcomes in three resolved windows:
//!
//!   baseline  the `baseline_secs` before the incident began
//!   incident  from the deploy the action reverted (found in the journal)
//!             to the action itself
//!   post      the last `window_secs` of ingested data, never earlier than
//!             the action, ending at the safe watermark
//!
//! and keeps the state machine per investigation:
//!
//!   clean       post rate within tolerance of the baseline
//!   recovered   `checks_required` consecutive clean checks -> the incident
//!               is CLOSED and a `verified_recovery` ledger entry is written
//!   worsening   post rate no better than the incident, or rising between
//!               checks while above tolerance -> ESCALATE immediately
//!   timeout     still open `timeout_secs` after the action -> ESCALATE;
//!               do not retry-storm
//!   not_recovered / insufficient_data -> wait and ask again
//!   too_soon    asked again within `interval_secs` of the last check: the
//!               streak does not move -- two checks on the same data are
//!               one check (an agent did exactly that in the first P9 run)
//!
//! Once closed or escalated, further checks return the same verdict; the
//! SOP's rule "never a second action" is a prompt rule, but the engine's
//! verdict is what the ledger records and what the benchmark scores.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spyglass_core::{Window, VerifyCfg};

use crate::{
    Engine,
    tools::{ChangepointArgs, ToolOutput, WindowArg, fmt_ts, pct},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyArgs {
    /// The service the action changed, e.g. "payments".
    pub service: String,
    /// The action's deploy id from the deployer's response (journal_entry.deploy_id), e.g. "D-3".
    pub deploy_id: String,
    /// Where recovery is judged: request lines of these services/instances count (default: every service; pass ["gateway"] to judge at the edge).
    #[serde(default)]
    pub services: Vec<String>,
}

/// Per-investigation verification state, keyed by deploy id.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VerifyState {
    pub checks: u32,
    pub consecutive_clean: u32,
    pub closed: bool,
    pub escalated: bool,
    pub escalation_reason: Option<String>,
    pub last_post_rate: Option<f64>,
    /// Wall clock of the last counted check; a check inside `interval_secs` of it is `too_soon`.
    pub last_check_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub baseline: f64,
    pub incident: f64,
    pub post: f64,
    pub post_requests: u64,
}

/// The decision, as a pure function so it is tested: what this check is,
/// given the rates, the state before it, the clock, and the config.
/// Returns (status, clean, escalate).
pub fn judge(r: &Rates, st: &VerifyState, elapsed_secs: i64, cfg: &VerifyCfg) -> (&'static str, bool, bool) {
    judge_at(r, st, elapsed_secs, i64::MAX, cfg)
}

/// `since_last_secs` is the wall-clock gap to the last counted check.
pub fn judge_at(r: &Rates, st: &VerifyState, elapsed_secs: i64, since_last_secs: i64, cfg: &VerifyCfg) -> (&'static str, bool, bool) {
    if st.closed {
        return ("recovered", true, false);
    }
    if st.escalated {
        return ("escalated", false, true);
    }
    // Consecutive checks must be separated in time (the spec's "every 15 s"):
    // a second call a moment later sees the same window and proves nothing.
    if st.checks > 0 && since_last_secs < cfg.interval_secs - 1 {
        return ("too_soon", false, false);
    }
    if r.post_requests < cfg.min_requests {
        return if elapsed_secs > cfg.timeout_secs { ("timeout", false, true) } else { ("insufficient_data", false, false) };
    }
    let tol = (r.baseline * cfg.tolerance_ratio).max(r.baseline + cfg.tolerance_abs);
    let clean = r.post <= tol;
    if clean {
        let n = st.consecutive_clean + 1;
        return if n >= cfg.checks_required { ("recovered", true, false) } else { ("clean", true, false) };
    }
    // Not clean. Worse than the incident itself, or rising across two dirty
    // checks: escalate now. One dirty check after a clean one is a wait, not
    // an escalation -- a single window can wobble.
    let rising = st.checks > 0 && st.consecutive_clean == 0 && st.last_post_rate.is_some_and(|prev| r.post > prev + cfg.tolerance_abs);
    if r.post >= r.incident.max(tol) || rising {
        return ("worsening", false, true);
    }
    if elapsed_secs > cfg.timeout_secs {
        return ("timeout", false, true);
    }
    ("not_recovered", false, false)
}

pub fn verify_recovery(engine: &Engine, investigation: &str, a: &VerifyArgs) -> Result<(ToolOutput, Value)> {
    let cfg = &engine.cfg;
    let vc = &cfg.verify;
    // ---- resolve the action and the windows (store lock) --------------------
    let (action, reverted, safe, baseline_w, incident_w, post_w, counts) = {
        let store = engine.store.read().expect("store lock");
        let action = store
            .deploys
            .iter()
            .filter(|d| d.deploy_id.as_deref() == Some(a.deploy_id.as_str()) && d.service == a.service)
            .max_by_key(|d| d.ts)
            .cloned()
            .ok_or_else(|| anyhow!("no journal entry {} for {} has been ingested yet; check freshness_watermark and retry", a.deploy_id, a.service))?;
        // The deploy the action reverted: the latest routing change of the
        // service to the version the action rolled back FROM, before the action.
        let reverted = store
            .deploys
            .iter()
            .filter(|d| d.deploy_id.is_some() && d.service == a.service && d.ts < action.ts)
            .filter(|d| action.from_version.is_some() && d.version == action.from_version)
            .max_by_key(|d| d.ts)
            .cloned();
        let safe = store.safe_log_ts().unwrap_or_else(Utc::now);
        let incident_from = reverted.as_ref().map(|d| d.ts).unwrap_or(action.ts - Duration::seconds(vc.incident_lookback_secs));
        let incident_w = Window { from: incident_from, to: action.ts };
        let baseline_w = Window { from: incident_from - Duration::seconds(vc.baseline_secs), to: incident_from };
        let post_from = (safe - Duration::seconds(vc.window_secs)).max(action.ts);
        let post_w = Window { from: post_from, to: safe.max(post_from) };
        #[derive(Default, Clone, Copy)]
        struct C { total: u64, errors: u64 }
        let mut c = [C::default(); 3];
        for e in store.events.iter().filter(|e| e.status.is_some() && e.route.is_some()) {
            if !a.services.is_empty() && !a.services.iter().any(|s| *s == e.service || *s == e.instance) {
                continue;
            }
            let err = e.status.is_some_and(|s| s >= 500) as u64;
            for (i, w) in [baseline_w, incident_w, post_w].iter().enumerate() {
                // post is (from, to]: an event at exactly the action instant belongs to the incident
                let inside = if i == 2 { e.ts > w.from && e.ts <= w.to } else { e.ts >= w.from && e.ts < w.to };
                if inside {
                    c[i].total += 1;
                    c[i].errors += err;
                }
            }
        }
        (action, reverted, safe, baseline_w, incident_w, post_w, c.map(|x| (x.total, x.errors)))
    };
    let rate = |(t, e): (u64, u64)| if t > 0 { e as f64 / t as f64 } else { 0.0 };
    let rates = Rates { baseline: rate(counts[0]), incident: rate(counts[1]), post: rate(counts[2]), post_requests: counts[2].0 };
    let now = Utc::now();
    let elapsed = (now - action.ts).num_seconds();
    let since_last = engine.with_investigation(investigation, |inv| {
        inv.verifications.get(&format!("{}:{}", a.service, a.deploy_id)).and_then(|s| s.last_check_at).map(|t| (now - t).num_seconds()).unwrap_or(i64::MAX)
    });

    // ---- the recovery changepoint: a `down` step on an error series after the action
    let (cps, _) = crate::tools::detect_changepoints(engine, &ChangepointArgs {
        metrics: vec!["error_rate".into()],
        service: None,
        route: None,
        window: WindowArg { from: Some(fmt_ts(incident_w.from)), to: Some(fmt_ts(safe)) },
        baseline: WindowArg { from: Some(fmt_ts(incident_w.from)), to: Some(fmt_ts(incident_w.to)) },
        limit: Some(cfg.bounds.max_items),
    })?;
    let recovery_cp = cps.payload["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["direction"] == "down")
        .filter(|c| c["at"].as_str().and_then(|t| t.parse::<DateTime<Utc>>().ok()).is_some_and(|t| t >= action.ts - Duration::seconds(cfg.changepoints.bucket_secs)))
        .map(|c| json!({"series": c["series"], "at": c["at"], "baseline_mean": c["baseline_mean"], "run_mean": c["run_mean"]}))
        .next();

    // ---- state machine (investigation lock) -----------------------------------
    let key = format!("{}:{}", a.service, a.deploy_id);
    let (status, clean, escalate, st, closed_now) = engine.with_investigation(investigation, |inv| {
        let st = inv.verifications.entry(key.clone()).or_default();
        let (status, clean, escalate) = judge_at(&rates, st, elapsed, since_last, vc);
        let was_closed = st.closed;
        if !st.closed && !st.escalated && status != "too_soon" {
            st.checks += 1;
            st.last_check_at = Some(now);
            // Too little data is neither clean nor dirty: the streak stands.
            if status != "insufficient_data" {
                if clean { st.consecutive_clean += 1 } else { st.consecutive_clean = 0 }
                st.last_post_rate = Some(rates.post);
            }
            if status == "recovered" {
                st.closed = true;
            }
            if escalate {
                st.escalated = true;
                st.escalation_reason = Some(status.to_string());
            }
        }
        (status, clean, escalate, st.clone(), st.closed && !was_closed)
    });
    let tol = (rates.baseline * vc.tolerance_ratio).max(rates.baseline + vc.tolerance_abs);
    let next = match status {
        "recovered" => "CLOSED: recovery verified; write the postmortem citing the verification eids".to_string(),
        "clean" => format!("clean ({}/{}); sleep {} s and call verify_recovery again", st.consecutive_clean, vc.checks_required, vc.interval_secs),
        "insufficient_data" => format!("only {} request(s) since the action; sleep {} s and call again", rates.post_requests, vc.interval_secs),
        "too_soon" => format!("not counted: {} s since the last check, checks must be {} s apart; sleep {} s and call again", since_last, vc.interval_secs, vc.interval_secs),
        "not_recovered" => format!("post rate {} above tolerance {}; sleep {} s and call again (escalates at {} s after the action)", pct(rates.post), pct(tol), vc.interval_secs, vc.timeout_secs),
        "worsening" => "ESCALATE to a human now: the rate is no better than the incident (or rising); take no further action".to_string(),
        "timeout" => format!("ESCALATE to a human: not recovered {} s after the action; take no further action", vc.timeout_secs),
        _ => "ESCALATED earlier; take no further action".to_string(),
    };
    let record = json!({
        "kind": "verification_check",
        "ref": format!("verify:{}:{}:{}", a.service, a.deploy_id, st.checks),
        "service": a.service, "deploy_id": a.deploy_id, "action_ts": fmt_ts(action.ts), "action_kind": action.kind,
        "reverted_deploy": reverted.as_ref().map(|d| json!({"deploy_id": d.deploy_id, "version": d.version, "ts": fmt_ts(d.ts)})),
        "check_n": st.checks, "status": status, "clean": clean, "counted": status != "too_soon", "since_last_check_secs": if since_last == i64::MAX { Value::Null } else { json!(since_last) },
        "consecutive_clean": st.consecutive_clean, "checks_required": vc.checks_required,
        "closed": st.closed, "closed_now": closed_now, "escalate": escalate, "escalation_reason": st.escalation_reason,
        "elapsed_secs": elapsed, "timeout_secs": vc.timeout_secs,
        "windows": {"baseline": baseline_w, "incident": incident_w, "post": post_w},
        "requests": {"baseline": counts[0].0, "incident": counts[1].0, "post": counts[2].0},
        "errors_5xx": {"baseline": counts[0].1, "incident": counts[1].1, "post": counts[2].1},
        "rates": {"baseline": r4(rates.baseline), "incident": r4(rates.incident), "post": r4(rates.post), "tolerance": r4(tol)},
        "judged_on": if a.services.is_empty() { json!("every service's request lines") } else { json!(a.services) },
        "recovery_changepoint": recovery_cp,
        "next": next,
    });
    let summary = format!(
        "verify_recovery {} {} check {}: {} (post {} over {} req vs baseline {} tol {}; incident {}){}{}",
        a.service, a.deploy_id, st.checks, status, pct(rates.post), counts[2].0, pct(rates.baseline), pct(tol), pct(rates.incident),
        if closed_now { " → CLOSED" } else { "" },
        if escalate { " → ESCALATE" } else { "" }
    );
    let resolved = json!({"service": a.service, "deploy_id": a.deploy_id, "services": a.services, "windows": {"baseline": baseline_w, "incident": incident_w, "post": post_w}});
    let mut payload = record.clone();
    if let Value::Object(m) = &mut payload {
        m.insert("items".into(), json!([record]));
    }
    Ok((ToolOutput { payload, summary, window: Some(post_w), deterministic: false, available: 1, records: None }, resolved))
}

fn r4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

pub fn states_json(m: &BTreeMap<String, VerifyState>) -> Value {
    json!(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> VerifyCfg {
        VerifyCfg::default()
    }
    fn r(baseline: f64, incident: f64, post: f64, n: u64) -> Rates {
        Rates { baseline, incident, post, post_requests: n }
    }

    #[test]
    fn two_consecutive_clean_checks_close_the_incident() {
        let c = cfg();
        let st = VerifyState::default();
        assert_eq!(judge(&r(0.0, 0.2, 0.0, 100), &st, 30, &c), ("clean", true, false));
        let st2 = VerifyState { checks: 1, consecutive_clean: 1, last_post_rate: Some(0.0), ..Default::default() };
        assert_eq!(judge(&r(0.0, 0.2, 0.01, 100), &st2, 45, &c), ("recovered", true, false));
    }

    #[test]
    fn a_dirty_check_resets_the_streak_and_waits() {
        let c = cfg();
        let st = VerifyState { checks: 1, consecutive_clean: 1, last_post_rate: Some(0.0), ..Default::default() };
        // 8% is above the 2-point tolerance but well under the 20% incident: not recovered, keep waiting
        assert_eq!(judge(&r(0.0, 0.2, 0.08, 100), &st, 45, &c), ("not_recovered", false, false));
    }

    #[test]
    fn no_better_than_the_incident_escalates_immediately() {
        let c = cfg();
        assert_eq!(judge(&r(0.0, 0.2, 0.2, 100), &VerifyState::default(), 20, &c), ("worsening", false, true));
        assert_eq!(judge(&r(0.0, 0.2, 0.35, 100), &VerifyState::default(), 20, &c), ("worsening", false, true));
    }

    #[test]
    fn rising_between_checks_while_dirty_escalates() {
        let c = cfg();
        let st = VerifyState { checks: 1, consecutive_clean: 0, last_post_rate: Some(0.05), ..Default::default() };
        assert_eq!(judge(&r(0.0, 0.2, 0.09, 100), &st, 40, &c), ("worsening", false, true));
    }

    #[test]
    fn the_timeout_escalates_instead_of_retrying_forever() {
        let c = cfg();
        assert_eq!(judge(&r(0.0, 0.2, 0.08, 100), &VerifyState::default(), 301, &c), ("timeout", false, true));
        assert_eq!(judge(&r(0.0, 0.2, 0.0, 3), &VerifyState::default(), 301, &c), ("timeout", false, true));
    }

    #[test]
    fn too_few_requests_is_not_a_verdict() {
        let c = cfg();
        assert_eq!(judge(&r(0.0, 0.2, 0.0, 5), &VerifyState::default(), 10, &c), ("insufficient_data", false, false));
    }

    #[test]
    fn tolerance_scales_with_a_noisy_baseline() {
        let c = cfg();
        // baseline 4%: tolerance is max(6%, 6%) = 6%; 5.5% is clean, 7% is not
        assert_eq!(judge(&r(0.04, 0.3, 0.055, 100), &VerifyState::default(), 10, &c).0, "clean");
        assert_eq!(judge(&r(0.04, 0.3, 0.07, 100), &VerifyState::default(), 10, &c).0, "not_recovered");
    }

    #[test]
    fn a_check_inside_the_interval_is_not_counted() {
        let c = cfg();
        let st = VerifyState { checks: 1, consecutive_clean: 1, last_post_rate: Some(0.0), ..Default::default() };
        assert_eq!(judge_at(&r(0.0, 0.2, 0.0, 500), &st, 30, 1, &c), ("too_soon", false, false));
        assert_eq!(judge_at(&r(0.0, 0.2, 0.0, 500), &st, 45, 15, &c), ("recovered", true, false));
        // the first check is never too soon
        assert_eq!(judge_at(&r(0.0, 0.2, 0.0, 500), &VerifyState::default(), 5, 0, &c), ("clean", true, false));
    }

    #[test]
    fn closed_and_escalated_are_terminal() {
        let c = cfg();
        let closed = VerifyState { closed: true, ..Default::default() };
        assert_eq!(judge(&r(0.0, 0.2, 0.5, 100), &closed, 10, &c), ("recovered", true, false));
        let esc = VerifyState { escalated: true, escalation_reason: Some("worsening".into()), ..Default::default() };
        assert_eq!(judge(&r(0.0, 0.2, 0.0, 100), &esc, 10, &c), ("escalated", false, true));
    }
}
