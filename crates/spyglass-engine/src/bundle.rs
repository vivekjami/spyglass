//! Evidence bundles (README C6): the one-call investigation starter.
//!
//! Candidates come from the same functions the individual tools use --
//! `novel_templates` (what is new), `detect_changepoints` (when it changed)
//! and the deploy journal (what was changed) -- over one frozen window, so
//! a bundle is deterministic on frozen data like every other read tool.
//! Then, in order:
//!
//!   1. dedupe: an error that propagates through the call chain is ONE
//!      fact. Templates whose exemplar events share request ids are a
//!      cascade (reported once, origin first, the rest listed inside);
//!      so are error-series changepoints on connected services within
//!      `cascade_secs` of each other
//!   2. score: the linear model in `rank.rs`, weights from config or the
//!      call, contributions kept on every item
//!   3. order: score desc, but the head is kind-diverse -- the best
//!      template, the best changepoint, the best deploy first, because an
//!      incident is what changed, when, and what was deployed; then the
//!      rest by score
//!   4. bound: `max_items` and `bundle.max_bytes`, enforced here. Items
//!      are compact views; the full record (with the excerpt) is what the
//!      evidence id dereferences to via `get_evidence`
//!   5. relate: deploy -> change events within the correlation window,
//!      changepoint <-> template that coincide
//!
//! `coverage` says how much was distilled into how little: that ratio is
//! the product metric.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use spyglass_core::{RankingCfg, Window, cap_item, sha256_hex};

use crate::{
    Engine,
    rank::{self, Factors, Score},
    tools::{ChangepointArgs, NoveltyArgs, ToolOutput, WindowArg, fmt_ts, resolve},
};

#[derive(Debug, Deserialize, JsonSchema, Clone, Default)]
#[schemars(inline)]
pub struct WeightsArg {
    pub w_n: Option<f64>,
    pub w_t: Option<f64>,
    pub w_s: Option<f64>,
    pub w_d: Option<f64>,
    pub w_f: Option<f64>,
    pub w_r: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BundleArgs {
    /// The incident window {from, to}. Default: the last 5 minutes of ingested data.
    #[serde(default)]
    pub window: WindowArg,
    /// The alerting service (e.g. "gateway"). Relevance decays per topology hop from it; omit for no focus.
    pub focus_service: Option<String>,
    /// Max items (default 20). The byte budget (8 kB) applies regardless.
    pub limit: Option<usize>,
    /// Ranking weight overrides for ablation runs, e.g. {"w_n": 0}. Recorded in the ledger with the result.
    #[serde(default)]
    pub weights: WeightsArg,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Template,
    Changepoint,
    Deploy,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Template => "novel_template",
            Kind::Changepoint => "changepoint",
            Kind::Deploy => "deploy",
        }
    }
}

/// One deduped fact, before scoring.
struct Fact {
    kind: Kind,
    /// Stable id: template_id | series key | deploy_id (relationships use these, not eids, so digests are session-independent).
    r#ref: String,
    /// The fact's own time: first_seen | at | deploy ts.
    t: DateTime<Utc>,
    services: BTreeSet<String>,
    factors: Factors,
    /// The compact item returned in the bundle (score fields added later).
    item: Value,
    /// The full evidence record the eid dereferences to.
    record: Value,
}

fn f(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0)
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}
fn ts(v: &Value, k: &str) -> Option<DateTime<Utc>> {
    v.get(k).and_then(|x| x.as_str()).and_then(|x| x.parse().ok())
}
fn strs(v: &Value, k: &str) -> BTreeSet<String> {
    v.get(k).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}
fn series_labels(series: &str) -> BTreeMap<String, String> {
    let inner = series.find('{').map(|i| &series[i + 1..series.len() - 1]).unwrap_or("");
    inner
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string()))
        .collect()
}

pub fn build_evidence_bundle(engine: &Engine, a: &BundleArgs) -> Result<(ToolOutput, Value)> {
    let cfg = &engine.cfg;
    let mut w: RankingCfg = cfg.ranking.clone();
    if let Some(x) = a.weights.w_n { w.w_n = x }
    if let Some(x) = a.weights.w_t { w.w_t = x }
    if let Some(x) = a.weights.w_s { w.w_s = x }
    if let Some(x) = a.weights.w_d { w.w_d = x }
    if let Some(x) = a.weights.w_f { w.w_f = x }
    if let Some(x) = a.weights.w_r { w.w_r = x }
    let limit = a.limit.unwrap_or(cfg.bounds.max_items).clamp(1, cfg.bounds.max_items);
    let corr = cfg.windows.deploy_correlation_secs as f64;

    // One frozen window for every candidate source.
    let (win, base, instance_service) = {
        let store = engine.store.read().expect("store lock");
        let watermark = store.safe_log_ts().unwrap_or_else(Utc::now);
        let win = match a.window.given() {
            Some(x) => resolve(&store, cfg, Some(x))?,
            None => Window::ending_at(watermark, cfg.bundle.incident_window_secs),
        };
        let base = Window { from: win.from - Duration::seconds(cfg.bundle.baseline_secs), to: win.from };
        let m: HashMap<String, String> = cfg.services.iter().map(|x| (x.name.clone(), x.service.clone())).collect();
        (win, base, m)
    };
    let warg = |x: Window| WindowArg { from: Some(fmt_ts(x.from)), to: Some(fmt_ts(x.to)) };
    let resolved = json!({"window": win, "baseline": base, "focus_service": a.focus_service, "limit": limit, "weights": w});

    // Candidates, through the tools' own functions (each takes the lock briefly).
    let (nov, _) = crate::tools::novel_templates(engine, &NoveltyArgs {
        window: warg(win), baseline: warg(base), min_score: None, limit: Some(cfg.bounds.max_items), services: vec![], level: None,
    })?;
    let (cps, _) = crate::tools::detect_changepoints(engine, &ChangepointArgs {
        metrics: vec![], service: None, route: None, window: warg(win), baseline: WindowArg::default(), limit: Some(cfg.bounds.max_items),
    })?;
    let templates: Vec<Value> = nov.payload["items"].as_array().cloned().unwrap_or_default();
    let changepoints: Vec<Value> = cps.payload["items"].as_array().cloned().unwrap_or_default();

    // Topology over logical services, for relevance and cascade connectivity.
    let svc_of = |name: &str| instance_service.get(name).cloned().unwrap_or_else(|| name.to_string());
    let edges: Vec<(String, String)> = cfg
        .services
        .iter()
        .flat_map(|x| x.upstreams.iter().map(move |u| (x.service.clone(), svc_of(u))))
        .collect();
    let hops: Option<HashMap<String, usize>> = a.focus_service.as_deref().map(|fs| rank::hop_distances(&svc_of(fs), &edges));
    let relevance_of = |services: &BTreeSet<String>| -> f64 {
        match &hops {
            None => 1.0,
            Some(h) => services.iter().map(|x| rank::relevance(h.get(&svc_of(x)).copied(), w.relevance_hop_decay)).fold(0.0, f64::max),
        }
    };
    let connected = |a: &str, b: &str| -> bool {
        let (a, b) = (svc_of(a), svc_of(b));
        a == b || rank::hop_distances(&a, &edges).contains_key(&b)
    };

    // Store-side facts: deploys, coverage, exemplar request ids.
    let (deploys, events_scanned, bytes_scanned, templates_in_window, req_ids_of): (Vec<spyglass_core::DeployEvent>, usize, usize, usize, HashMap<String, BTreeSet<String>>) = {
        let store = engine.store.read().expect("store lock");
        let referenced: BTreeSet<String> = changepoints
            .iter()
            .filter_map(|c| c.get("nearest_deploy").and_then(|d| d.get("deploy_id")).and_then(|x| x.as_str()).map(str::to_string))
            .collect();
        let deploys: Vec<_> = store
            .deploys
            .iter()
            .filter(|d| d.deploy_id.is_some() && d.ts <= win.to)
            .filter(|d| win.contains(d.ts) || referenced.contains(d.deploy_id.as_deref().unwrap_or("")))
            .cloned()
            .collect();
        let mut n = 0usize;
        let mut bytes = 0usize;
        let mut tids: HashSet<&str> = HashSet::new();
        for e in store.events.iter().filter(|e| win.contains(e.ts)) {
            n += 1;
            bytes += e.raw.len();
            tids.insert(e.template_id.as_str());
        }
        let mut req: HashMap<String, BTreeSet<String>> = HashMap::new();
        for t in &templates {
            let ids = strs(t, "exemplar_event_ids");
            let set: BTreeSet<String> = ids
                .iter()
                .filter_map(|id| store.by_event_id.get(id).map(|&i| &store.events[i]))
                .filter_map(|e| e.req_id.clone())
                .collect();
            req.insert(s(t, "template_id").to_string(), set);
        }
        (deploys, n, bytes, tids.len(), req)
    };

    // The onset estimate T0: the earliest error changepoint, else the
    // earliest novel ERROR template, else the window end.
    let is_error_cp = |c: &Value| matches!(s(c, "metric"), "error_rate" | "errors_total") && s(c, "direction") == "up";
    let (t0, t0_source) = changepoints
        .iter()
        .filter(|c| is_error_cp(c))
        .filter_map(|c| ts(c, "at"))
        .min()
        .map(|t| (t, "earliest_error_changepoint"))
        .or_else(|| {
            templates
                .iter()
                .filter(|t| rank::severity_of_level(s(t, "dominant_level")) >= 1.0)
                .filter_map(|t| ts(t, "first_seen_in_window"))
                .min()
                .map(|t| (t, "earliest_novel_error_template"))
        })
        .unwrap_or((win.to, "window_end"));
    let prox = |t: DateTime<Utc>| rank::proximity((t - t0).num_milliseconds() as f64 / 1000.0, w.proximity_tau_secs);
    let deploy_near = |t: DateTime<Utc>| deploys.iter().any(|d| ((t - d.ts).num_milliseconds() as f64 / 1000.0).abs() <= corr);
    let change_times: Vec<DateTime<Utc>> = changepoints.iter().filter_map(|c| ts(c, "at")).chain(templates.iter().filter_map(|t| ts(t, "first_seen_in_window"))).collect();

    let mut facts: Vec<Fact> = Vec::new();

    // ---- templates: cascade by shared request ids -------------------------
    let n_t = templates.len();
    let mut parent: Vec<usize> = (0..n_t).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        if p[i] != i {
            let r = find(p, p[i]);
            p[i] = r;
        }
        p[i]
    }
    for i in 0..n_t {
        for j in i + 1..n_t {
            let (a, b) = (&req_ids_of[s(&templates[i], "template_id")], &req_ids_of[s(&templates[j], "template_id")]);
            if !a.is_empty() && a.intersection(b).next().is_some() {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n_t {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    for (_, mut members) in groups {
        // origin: earliest first_seen, then has_stack, then severity
        members.sort_by(|&x, &y| {
            let (tx, ty) = (&templates[x], &templates[y]);
            ts(tx, "first_seen_in_window").cmp(&ts(ty, "first_seen_in_window"))
                .then(tx["has_stack"].as_bool().cmp(&ty["has_stack"].as_bool()).reverse())
                .then(rank::severity_of_level(s(ty, "dominant_level")).partial_cmp(&rank::severity_of_level(s(tx, "dominant_level"))).unwrap_or(std::cmp::Ordering::Equal))
        });
        let root = &templates[members[0]];
        let first_seen = ts(root, "first_seen_in_window").unwrap_or(win.to);
        let mut services: BTreeSet<String> = strs(root, "services");
        let cascade: Vec<Value> = members[1..]
            .iter()
            .map(|&i| {
                let t = &templates[i];
                services.extend(strs(t, "services"));
                json!({"ref": s(t, "template_id"), "pattern": s(t, "pattern"), "level": s(t, "dominant_level"), "services": t["services"],
                       "first_seen": s(t, "first_seen_in_window"), "count": t["count_window"], "has_stack": t["has_stack"]})
            })
            .collect();
        let level = s(root, "dominant_level").to_string();
        let ratio = root.get("burst_ratio").and_then(|x| x.as_f64());
        let factors = Factors {
            novelty: f(root, "novelty"),
            proximity: prox(first_seen),
            severity: rank::severity_of_level(&level),
            deploy_correlation: if deploy_near(first_seen) { 1.0 } else { 0.0 },
            freq_shift: if s(root, "novelty_reason") == "first_seen_in_window" { 1.0 } else { rank::freq_shift_of_ratio(ratio, cfg.novelty.burst_log2_scale) },
            relevance: relevance_of(&services),
        };
        let excerpt_bytes = s(root, "excerpt").len();
        let item = json!({
            "kind": "novel_template", "ref": s(root, "template_id"),
            "pattern": s(root, "pattern"), "level": level, "has_stack": root["has_stack"],
            "novelty": root["novelty"], "novelty_reason": root["novelty_reason"],
            "first_seen": s(root, "first_seen_in_window"), "count": root["count_window"], "count_baseline": root["count_baseline"],
            "services": root["services"], "instances": root["instances"],
            "cascade": cascade,
            "exemplar_event_ids": root["exemplar_event_ids"], "excerpt_bytes": excerpt_bytes,
        });
        let mut record = root.clone();
        if let Value::Object(m) = &mut record {
            m.insert("cascade".into(), item["cascade"].clone());
            m.insert("bundle_ref".into(), Value::String(s(root, "template_id").to_string()));
        }
        facts.push(Fact { kind: Kind::Template, r#ref: s(root, "template_id").to_string(), t: first_seen, services, factors, item, record });
    }

    // ---- changepoints: error cascades on connected services ----------------
    let mut used = vec![false; changepoints.len()];
    let mut order: Vec<usize> = (0..changepoints.len()).collect();
    order.sort_by_key(|&i| ts(&changepoints[i], "at"));
    let cp_service = |c: &Value| -> String {
        let l = series_labels(s(c, "series"));
        l.get("service").cloned().or_else(|| l.get("instance").map(|i| svc_of(i))).unwrap_or_default()
    };
    for &i in &order {
        if used[i] {
            continue;
        }
        used[i] = true;
        let origin = &changepoints[i];
        let at = ts(origin, "at").unwrap_or(win.to);
        let mut services: BTreeSet<String> = BTreeSet::from([cp_service(origin)]);
        let mut cascade: Vec<Value> = Vec::new();
        if is_error_cp(origin) {
            for &j in &order {
                if used[j] || !is_error_cp(&changepoints[j]) {
                    continue;
                }
                let c = &changepoints[j];
                let dt = (ts(c, "at").unwrap_or(win.to) - at).num_milliseconds() as f64 / 1000.0;
                if dt.abs() <= w.cascade_secs && connected(&cp_service(origin), &cp_service(c)) {
                    used[j] = true;
                    services.insert(cp_service(c));
                    cascade.push(json!({"series": s(c, "series"), "at": s(c, "at"), "run_mean": c["run_mean"], "magnitude_x": c["magnitude_x"]}));
                }
            }
        }
        let metric = s(origin, "metric");
        let dir = s(origin, "direction");
        let severity = match (metric, dir) {
            ("error_rate" | "errors_total", "up") => 1.0,
            ("latency_ms_mean", "up") => 0.6,
            _ => 0.3,
        };
        let (bm, rm) = (f(origin, "baseline_mean"), f(origin, "run_mean"));
        let ratio = match dir {
            "up" => if bm > 0.0 { Some(rm / bm) } else { None },
            _ => if rm > 0.0 { Some(bm / rm) } else { None },
        };
        // Novelty is defined the same way for every kind: did this behaviour
        // first appear in the window? A template first seen there scores 1.0
        // and so does an error series going from zero; a burst scores by the
        // shared log2 mapping. (freq_shift is the magnitude; for a from-zero
        // step the two coincide, exactly as they do for a first-seen template.)
        let shift = rank::freq_shift_of_ratio(ratio, cfg.novelty.burst_log2_scale);
        let factors = Factors {
            novelty: shift,
            proximity: prox(at),
            severity,
            deploy_correlation: if origin.get("nearest_deploy").is_some_and(|d| !d.is_null()) { 1.0 } else { 0.0 },
            freq_shift: shift,
            relevance: relevance_of(&services),
        };
        let nd = origin.get("nearest_deploy").cloned().unwrap_or(Value::Null);
        let item = json!({
            "kind": "changepoint", "ref": s(origin, "series"),
            "series": s(origin, "series"), "direction": dir, "at": s(origin, "at"), "at_precision": s(origin, "at_precision"),
            "baseline_mean": origin["baseline_mean"], "run_mean": origin["run_mean"], "magnitude_x": origin["magnitude_x"], "z_peak": origin["z_peak"],
            "nearest_deploy": if nd.is_null() { Value::Null } else { json!({"deploy_id": nd["deploy_id"], "offset_secs": nd["offset_secs"], "relation": nd["relation"]}) },
            "cascade": cascade,
            "also_changed": origin["also_changed"],
        });
        let mut record = origin.clone();
        if let Value::Object(m) = &mut record {
            m.insert("cascade".into(), item["cascade"].clone());
            m.insert("bundle_ref".into(), Value::String(s(origin, "series").to_string()));
        }
        facts.push(Fact { kind: Kind::Changepoint, r#ref: s(origin, "series").to_string(), t: at, services, factors, item, record });
    }

    // ---- deploys -------------------------------------------------------------
    for d in &deploys {
        let did = d.deploy_id.clone().unwrap_or_default();
        let services = BTreeSet::from([d.service.clone()]);
        let correlated = change_times.iter().any(|t| ((*t - d.ts).num_milliseconds() as f64 / 1000.0).abs() <= corr);
        let factors = Factors {
            novelty: 1.0,
            proximity: prox(d.ts),
            severity: 0.5,
            deploy_correlation: if correlated { 1.0 } else { 0.0 },
            freq_shift: 0.0,
            relevance: relevance_of(&services),
        };
        let item = json!({
            "kind": "deploy", "ref": did, "deploy_id": did, "journal_kind": d.kind, "service": d.service,
            "from_version": d.from_version, "version": d.version, "ts": fmt_ts(d.ts), "actor": d.actor,
        });
        let mut record = serde_json::to_value(d).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut record {
            m.insert("kind".into(), Value::String("deploy".into()));
            m.insert("journal_kind".into(), Value::String(d.kind.clone()));
            m.insert("bundle_ref".into(), Value::String(did.clone()));
        }
        facts.push(Fact { kind: Kind::Deploy, r#ref: did, t: d.ts, services, factors, item, record });
    }

    // ---- score, order, bound ---------------------------------------------------
    let scored: Vec<(Score, usize)> = facts.iter().enumerate().map(|(i, x)| (rank::score(&x.factors, &w), i)).collect();
    let by_score = |x: &(Score, usize), y: &(Score, usize)| {
        y.0.total.partial_cmp(&x.0.total).unwrap_or(std::cmp::Ordering::Equal)
            .then(facts[x.1].kind.cmp(&facts[y.1].kind))
            .then(facts[x.1].t.cmp(&facts[y.1].t))
            .then(facts[x.1].r#ref.cmp(&facts[y.1].r#ref))
    };
    let mut rest = scored.clone();
    rest.sort_by(by_score);
    // kind-diverse head: the best of each kind present, in score order
    let mut head: Vec<(Score, usize)> = Vec::new();
    for k in [Kind::Template, Kind::Changepoint, Kind::Deploy] {
        if let Some(best) = rest.iter().find(|(_, i)| facts[*i].kind == k).cloned() {
            head.push(best);
        }
    }
    head.sort_by(by_score);
    let head_idx: HashSet<usize> = head.iter().map(|x| x.1).collect();
    // Pure-score rank, kept on every item so the head's effect is visible.
    let score_rank: HashMap<usize, usize> = rest.iter().enumerate().map(|(pos, (_, i))| (*i, pos + 1)).collect();
    let ordered: Vec<(Score, usize)> = head.into_iter().chain(rest.into_iter().filter(|x| !head_idx.contains(&x.1))).collect();
    let available = ordered.len();

    let budget = cfg.bundle.max_bytes.saturating_sub(1400); // envelope + relationships reserve
    let mut items: Vec<Value> = Vec::new();
    let mut records: Vec<Value> = Vec::new();
    let mut chosen: Vec<usize> = Vec::new();
    let mut used_bytes = 0usize;
    for (sc, i) in ordered.iter() {
        if items.len() >= limit {
            break;
        }
        let fct = &facts[*i];
        let mut item = fct.item.clone();
        if let Value::Object(m) = &mut item {
            m.insert("score".into(), json!(sc.total));
            m.insert("score_rank".into(), json!(score_rank.get(i).copied().unwrap_or(0)));
            m.insert("factors".into(), json!({"n": sc.n, "t": sc.t, "s": sc.s, "d": sc.d, "f": sc.f, "r": sc.r}));
        }
        cap_item(&mut item, cfg.bounds.max_bytes_per_item);
        let sz = serde_json::to_vec(&item).map(|b| b.len()).unwrap_or(0) + 1;
        if used_bytes + sz > budget {
            break;
        }
        used_bytes += sz;
        items.push(item);
        let mut rec = fct.record.clone();
        if let Value::Object(m) = &mut rec {
            m.insert("score".into(), json!(sc.total));
            m.insert("factors".into(), json!(fct.factors));
        }
        records.push(rec);
        chosen.push(*i);
    }

    // ---- relationships among included items --------------------------------
    let mut rels: Vec<Value> = Vec::new();
    for &i in &chosen {
        if facts[i].kind != Kind::Deploy {
            continue;
        }
        for &j in &chosen {
            if facts[j].kind == Kind::Deploy {
                continue;
            }
            let off = (facts[j].t - facts[i].t).num_milliseconds() as f64 / 1000.0;
            if (0.0..=corr).contains(&off) {
                rels.push(json!({"from": facts[i].r#ref, "to": facts[j].r#ref, "type": format!("precedes_within_{}s", corr as i64), "offset_secs": (off * 10.0).round() / 10.0}));
            }
        }
    }
    for &i in &chosen {
        if facts[i].kind != Kind::Changepoint {
            continue;
        }
        for &j in &chosen {
            if facts[j].kind != Kind::Template {
                continue;
            }
            let off = (facts[j].t - facts[i].t).num_milliseconds() as f64 / 1000.0;
            if off.abs() <= w.cascade_secs {
                rels.push(json!({"from": facts[i].r#ref, "to": facts[j].r#ref, "type": format!("coincides_within_{}s", w.cascade_secs), "offset_secs": (off * 10.0).round() / 10.0}));
            }
        }
    }
    rels.sort_by(|x, y| s(x, "type").cmp(s(y, "type")).then(s(x, "from").cmp(s(y, "from"))).then(s(x, "to").cmp(s(y, "to"))));

    let bundle_id = format!("B-{}", &sha256_hex(serde_json::to_string(&resolved).unwrap_or_default().as_bytes())[..12]);
    let assemble = |items: &Vec<Value>, rels: &Vec<Value>, truncated: bool| -> Value {
        let items_returned = items.len();
        let bytes_returned = serde_json::to_vec(&json!({"items": items, "relationships": rels})).map(|b| b.len()).unwrap_or(0);
        json!({
            "bundle_id": bundle_id,
            "window": win, "baseline": base,
            "focus_service": a.focus_service,
            "incident_t0": {"ts": fmt_ts(t0), "source": t0_source},
            "items": items,
            "relationships": rels,
            "coverage": {
                "events_scanned": events_scanned, "bytes_scanned": bytes_scanned,
                "templates_in_window": templates_in_window, "templates_novel": n_t,
                "changepoints_found": changepoints.len(), "deploys_considered": deploys.len(),
                "facts_after_dedupe": available, "items_returned": items_returned, "truncated": truncated,
                "bytes_returned": bytes_returned,
                "reduction_ratio": if items_returned > 0 { json!((events_scanned as f64 / items_returned as f64).round()) } else { Value::Null },
                "bytes_reduction_ratio": if bytes_returned > 0 { json!((bytes_scanned as f64 / bytes_returned as f64 * 10.0).round() / 10.0) } else { Value::Null },
            },
            "ranking": {"model": "linear_v0", "weights": w, "order": "kind-diverse head (best template, changepoint, deploy), then score desc; ties: kind, time asc, ref; score_rank on each item is its position by score alone"},
            "note": "items are compact; get_evidence(eid) returns the full record with the raw excerpt. Cascades are one fact: the origin is the item, the rest are listed in `cascade`. Deploy offsets are correlation, not cause.",
        })
    };
    let mut payload = assemble(&items, &rels, available > items.len());
    // Hard byte bound on the whole payload: drop from the tail until it fits.
    while serde_json::to_vec(&payload).map(|b| b.len()).unwrap_or(0) > cfg.bundle.max_bytes && !items.is_empty() {
        items.pop();
        records.pop();
        chosen.pop();
        let keep: HashSet<&str> = chosen.iter().map(|&i| facts[i].r#ref.as_str()).collect();
        let rels2: Vec<Value> = rels.iter().filter(|r| keep.contains(s(r, "from")) && keep.contains(s(r, "to"))).cloned().collect();
        payload = assemble(&items, &rels2, true);
    }

    let top: Vec<String> = items.iter().take(3).map(|it| match s(it, "kind") {
        "novel_template" => format!("T {} [{}] {:.2}", s(it, "pattern"), s(it, "level"), f(it, "score")),
        "changepoint" => format!("CP {} {} {:.2}", s(it, "series"), s(it, "direction"), f(it, "score")),
        _ => format!("D {} {} {}→{} {:.2}", s(it, "deploy_id"), s(it, "service"), s(it, "from_version"), s(it, "version"), f(it, "score")),
    }).collect();
    let cov = &payload["coverage"];
    let summary = format!(
        "build_evidence_bundle → {} items / {} B from {} events ({} B) in window; T0 {} ({}); top: {}",
        items.len(), cov["bytes_returned"], events_scanned, bytes_scanned, fmt_ts(t0), t0_source, top.join(" | ")
    );
    Ok((ToolOutput { payload, summary, window: Some(win), deterministic: true, available, records: Some(records) }, resolved))
}
