//! Evidence ranking (README C5, ADR-008): a hand-weighted linear model over
//! six factors, each in [0, 1], as a pure function so every score can be
//! explained factor by factor and unit-tested without a store.
//!
//!   score = w_n*novelty + w_t*proximity + w_s*severity
//!         + w_d*deploy_correlation + w_f*freq_shift + w_r*relevance
//!
//! The factors are defined per evidence kind (see `Factors` docs); the
//! weights live in `spyglass.toml [ranking]` and are recorded in the ledger
//! args of every bundle call, so a ranking is never a mystery after the fact.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::Serialize;
use spyglass_core::RankingCfg;

/// The six factor values for one candidate, all in [0, 1].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Factors {
    /// Did this behaviour first appear in the window? Templates: the
    /// novelty score. Changepoints: 1.0 from zero, else the burst mapping of
    /// the magnitude. Deploys: 1.0 (a deploy is a new event in its window).
    pub novelty: f64,
    /// exp(-|t - T0| / tau), T0 = the engine's onset estimate.
    pub proximity: f64,
    /// Templates: ERROR 1.0 / WARN 0.5 / INFO 0.0. Changepoints: error
    /// series up 1.0, latency 0.6, traffic 0.3, any down step 0.3. Deploys 0.5.
    pub severity: f64,
    /// 1.0 when a change event of another kind lies within the deploy
    /// correlation window, else 0.
    pub deploy_correlation: f64,
    /// min(1, log2(rate ratio) / scale); "from zero" / first seen = 1.0.
    pub freq_shift: f64,
    /// decay^hops from the focus service (max over a cascade's members);
    /// 1.0 for everything when there is no focus.
    pub relevance: f64,
}

/// A scored candidate: the total and each weighted contribution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Score {
    pub total: f64,
    pub n: f64,
    pub t: f64,
    pub s: f64,
    pub d: f64,
    pub f: f64,
    pub r: f64,
}

pub fn score(f: &Factors, w: &RankingCfg) -> Score {
    let c = |x: f64| x.clamp(0.0, 1.0);
    let (n, t, s, d, fr, r) = (
        w.w_n * c(f.novelty),
        w.w_t * c(f.proximity),
        w.w_s * c(f.severity),
        w.w_d * c(f.deploy_correlation),
        w.w_f * c(f.freq_shift),
        w.w_r * c(f.relevance),
    );
    let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
    Score {
        total: r3(n + t + s + d + fr + r),
        n: r3(n),
        t: r3(t),
        s: r3(s),
        d: r3(d),
        f: r3(fr),
        r: r3(r),
    }
}

pub fn proximity(delta_secs: f64, tau: f64) -> f64 {
    (-(delta_secs.abs()) / tau.max(1e-9)).exp()
}

pub fn severity_of_level(level: &str) -> f64 {
    match level {
        "CRITICAL" | "FATAL" | "ERROR" => 1.0,
        "WARNING" | "WARN" => 0.5,
        _ => 0.0,
    }
}

/// Burst-style mapping shared with novelty: 64x -> 1.0 at scale 6.
pub fn freq_shift_of_ratio(ratio: Option<f64>, scale: f64) -> f64 {
    match ratio {
        None => 1.0, // from zero / first seen
        Some(r) if r > 1.0 => (r.log2() / scale).clamp(0.0, 1.0),
        Some(_) => 0.0,
    }
}

/// Hop distances from `focus` over the undirected service graph (edges are
/// the config's `upstreams`). Instances resolve to their logical service.
pub fn hop_distances(focus: &str, edges: &[(String, String)]) -> HashMap<String, usize> {
    let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (a, b) in edges {
        adj.entry(a.as_str()).or_default().push(b.as_str());
        adj.entry(b.as_str()).or_default().push(a.as_str());
    }
    let mut dist: HashMap<String, usize> = HashMap::new();
    let mut q = VecDeque::new();
    dist.insert(focus.to_string(), 0);
    q.push_back(focus);
    while let Some(u) = q.pop_front() {
        let du = dist[u];
        for v in adj.get(u).into_iter().flatten() {
            if !dist.contains_key(*v) {
                dist.insert((*v).to_string(), du + 1);
                q.push_back(v);
            }
        }
    }
    dist
}

pub fn relevance(hops: Option<usize>, decay: f64) -> f64 {
    match hops {
        Some(h) => decay.powi(h as i32),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> RankingCfg {
        RankingCfg {
            w_n: 0.30,
            w_t: 0.15,
            w_s: 0.10,
            w_d: 0.25,
            w_f: 0.10,
            w_r: 0.10,
            proximity_tau_secs: 120.0,
            relevance_hop_decay: 0.75,
            cascade_secs: 2.0,
        }
    }
    fn edges() -> Vec<(String, String)> {
        [
            ("gateway", "orders"),
            ("orders", "payments"),
            ("orders", "postgres"),
            ("payments", "redis"),
            ("loadgen", "gateway"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
    }
    // S1's three key facts and its decoy, focus = gateway (the alerting service).
    fn root_template() -> Factors {
        Factors {
            novelty: 1.0,
            proximity: 1.0,
            severity: 1.0,
            deploy_correlation: 1.0,
            freq_shift: 1.0,
            relevance: 1.0,
        }
    }
    fn error_changepoint() -> Factors {
        Factors {
            novelty: 1.0,
            proximity: 1.0,
            severity: 1.0,
            deploy_correlation: 1.0,
            freq_shift: 1.0,
            relevance: 1.0,
        }
    }
    fn fault_deploy() -> Factors {
        Factors {
            novelty: 1.0,
            proximity: 0.996,
            severity: 0.5,
            deploy_correlation: 1.0,
            freq_shift: 0.0,
            relevance: 0.5625,
        }
    }
    fn info_decoy() -> Factors {
        Factors {
            novelty: 1.0,
            proximity: 1.0,
            severity: 0.0,
            deploy_correlation: 1.0,
            freq_shift: 1.0,
            relevance: 0.5625,
        }
    }
    fn benign_deploy() -> Factors {
        Factors {
            novelty: 1.0,
            proximity: proximity(360.0, 120.0),
            severity: 0.5,
            deploy_correlation: 0.0,
            freq_shift: 0.0,
            relevance: 0.75,
        }
    }

    #[test]
    fn the_root_template_outranks_everything_and_contributions_sum_to_the_total() {
        let s = score(&root_template(), &w());
        assert_eq!(s.total, 1.0);
        assert!((s.n + s.t + s.s + s.d + s.f + s.r - s.total).abs() < 1e-9);
        // the error changepoint from zero ties the template on every factor; the deploy trails on severity and freq_shift
        assert_eq!(score(&error_changepoint(), &w()).total, s.total);
        assert!(s.total > score(&fault_deploy(), &w()).total);
    }

    #[test]
    fn the_benign_deploy_six_minutes_out_ranks_below_the_three_key_facts() {
        let b = score(&benign_deploy(), &w()).total;
        for f in [root_template(), error_changepoint(), fault_deploy()] {
            assert!(score(&f, &w()).total > b, "{b}");
        }
    }

    #[test]
    fn the_info_decoy_is_not_separated_from_the_deploy_by_severity_alone() {
        // Recorded, not hidden: with w_s = 0.10 a novel INFO template scores
        // above the fault deploy on the linear model. The bundle's
        // kind-diverse head is what keeps the deploy in the top three.
        assert!(score(&info_decoy(), &w()).total > score(&fault_deploy(), &w()).total);
        assert!(score(&info_decoy(), &w()).total < score(&root_template(), &w()).total);
    }

    #[test]
    fn zeroing_the_novelty_weight_removes_exactly_w_n_from_every_novel_item() {
        let mut w0 = w();
        w0.w_n = 0.0;
        for f in [
            root_template(),
            error_changepoint(),
            fault_deploy(),
            info_decoy(),
        ] {
            assert!((score(&f, &w()).total - score(&f, &w0).total - 0.30).abs() < 1e-9);
        }
        // novelty is a constant across first-seen items, so it separates none
        // of them from each other -- what separates the decoy from the deploy is
        // severity and freq_shift; what puts the deploy in the bundle's top
        // three is the kind-diverse head, not the weights (docs/phase6-findings F2)
        let gap =
            |wx: &RankingCfg| score(&info_decoy(), wx).total - score(&fault_deploy(), wx).total;
        assert!((gap(&w()) - gap(&w0)).abs() < 1e-9);
    }

    #[test]
    fn relevance_decays_per_hop_and_is_zero_off_graph() {
        let d = hop_distances("gateway", &edges());
        assert_eq!(
            (d["gateway"], d["orders"], d["payments"], d["redis"]),
            (0, 1, 2, 3)
        );
        assert_eq!(relevance(d.get("payments").copied(), 0.75), 0.5625);
        assert_eq!(relevance(None, 0.75), 0.0);
    }

    #[test]
    fn freq_shift_matches_the_novelty_burst_mapping() {
        assert_eq!(freq_shift_of_ratio(None, 6.0), 1.0);
        assert_eq!(freq_shift_of_ratio(Some(8.0), 6.0), 0.5);
        assert_eq!(freq_shift_of_ratio(Some(1.0), 6.0), 0.0);
        assert_eq!(freq_shift_of_ratio(Some(128.0), 6.0), 1.0);
    }
}
