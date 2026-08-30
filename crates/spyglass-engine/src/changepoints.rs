//! Changepoint detection (README C4, ADR-007): rolling z-score on 10 s
//! aggregates with a guarded baseline, as a pure function over bucketed
//! series so it can be unit-tested without a store.
//!
//! The series are derived from the request events in the logs (every
//! `request completed` / `request failed` line carries service, instance,
//! route, status, latency_ms), not from the scraped Prometheus counters:
//! the log-derived series are event-time stamped and rebuilt from the files
//! on every engine start, so a changepoint is deterministic on frozen data
//! and its ledger entry re-checks (ADR-004). The scraped counters are
//! wall-clock stamped and live in an in-memory ring. ADR-007 records this.
//!
//! Definitions (all thresholds in `spyglass.toml [changepoints]`):
//!   * bucket i covers [t0 + i*B, t0 + (i+1)*B); buckets are aligned to
//!     multiples of B since the epoch, so identity does not depend on the
//!     query window
//!   * rolling baseline for bucket i = the defined values of buckets
//!     [i - baseline, i - guard); the guard keeps a change out of its own
//!     baseline while it is being confirmed
//!   * z_i = (x_i - mean) / max(sigma, floor); fewer than
//!     `min_baseline_buckets` defined baseline values -> z undetermined
//!   * a changepoint is a run of >= `consecutive_buckets` buckets flagged
//!     |z| >= threshold with one sign; it is reported once, at the run's
//!     first bucket

use std::collections::BTreeMap;

use spyglass_core::ChangepointCfg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Count,
    Rate,
    Latency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }
}

/// One confirmed changepoint on one series, in bucket indices.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// Index of the first flagged bucket: the changepoint.
    pub start: usize,
    /// Number of consecutive flagged buckets (same sign).
    pub len: usize,
    pub direction: Direction,
    pub z_first: f64,
    pub z_peak: f64,
    pub value_first: f64,
    pub run_mean: f64,
    pub baseline_mean: f64,
    pub baseline_sigma: f64,
    pub sigma_used: f64,
    pub baseline_buckets: usize,
    /// Baseline bucket index range [from, to) used for the first bucket.
    pub baseline_range: (usize, usize),
}

/// Which buckets a bucket's baseline is drawn from.
#[derive(Clone, Copy, Debug)]
pub enum Baseline {
    /// [i - baseline_buckets, i - guard_buckets)
    Rolling,
    /// A fixed index range [from, to) for every bucket -- e.g. the incident
    /// period, when asking "has it recovered?"
    Explicit(usize, usize),
}

fn sigma_floor(kind: Kind, mean: f64, cfg: &ChangepointCfg) -> f64 {
    match kind {
        Kind::Count => cfg.sigma_floor_count.max(mean.max(0.0).sqrt()),
        Kind::Rate => cfg.sigma_floor_rate,
        Kind::Latency => cfg
            .sigma_floor_latency_ms
            .max(cfg.sigma_floor_latency_frac * mean),
    }
}

struct Stats {
    mean: f64,
    sigma: f64,
    sigma_used: f64,
    n: usize,
    range: (usize, usize),
}

fn stats(
    values: &[Option<f64>],
    range: (usize, usize),
    kind: Kind,
    cfg: &ChangepointCfg,
) -> Option<Stats> {
    let (from, to) = range;
    if to <= from {
        return None;
    }
    let xs: Vec<f64> = values[from..to].iter().flatten().copied().collect();
    if xs.len() < cfg.min_baseline_buckets {
        return None;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    let sigma = var.sqrt();
    Some(Stats {
        mean,
        sigma,
        sigma_used: sigma.max(sigma_floor(kind, mean, cfg)),
        n: xs.len(),
        range,
    })
}

/// z per bucket (None where the value or its baseline is undefined).
fn zscores(
    values: &[Option<f64>],
    kind: Kind,
    baseline: Baseline,
    cfg: &ChangepointCfg,
) -> Vec<Option<(f64, Stats)>> {
    let b = (cfg.baseline_secs / cfg.bucket_secs).max(1) as usize;
    let g = (cfg.guard_secs / cfg.bucket_secs).max(1) as usize;
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = (*v)?;
            let range = match baseline {
                Baseline::Rolling => (i.saturating_sub(b), i.saturating_sub(g).min(i)),
                Baseline::Explicit(f, t) => (f.min(values.len()), t.min(values.len())),
            };
            let st = stats(values, range, kind, cfg)?;
            Some(((x - st.mean) / st.sigma_used, st))
        })
        .collect()
}

/// The detector: confirmed runs of flagged buckets, one `Run` per changepoint.
pub fn detect(
    values: &[Option<f64>],
    kind: Kind,
    baseline: Baseline,
    cfg: &ChangepointCfg,
) -> Vec<Run> {
    let zs = zscores(values, kind, baseline, cfg);
    let mut runs = Vec::new();
    let mut i = 0;
    while i < zs.len() {
        let Some((z, _)) = zs[i].as_ref() else {
            i += 1;
            continue;
        };
        if z.abs() < cfg.z_threshold {
            i += 1;
            continue;
        }
        let dir = if *z > 0.0 {
            Direction::Up
        } else {
            Direction::Down
        };
        let mut j = i;
        let mut z_peak = *z;
        let mut sum = 0.0;
        while j < zs.len() {
            match zs[j].as_ref() {
                Some((zj, _))
                    if zj.abs() >= cfg.z_threshold && (*zj > 0.0) == (dir == Direction::Up) =>
                {
                    if zj.abs() > z_peak.abs() {
                        z_peak = *zj;
                    }
                    sum += values[j].unwrap_or(0.0);
                    j += 1;
                }
                _ => break,
            }
        }
        let len = j - i;
        if len >= cfg.consecutive_buckets {
            let (z_first, st) = zs[i].as_ref().expect("flagged bucket has stats");
            runs.push(Run {
                start: i,
                len,
                direction: dir,
                z_first: *z_first,
                z_peak,
                value_first: values[i].unwrap_or(0.0),
                run_mean: sum / len as f64,
                baseline_mean: st.mean,
                baseline_sigma: st.sigma,
                sigma_used: st.sigma_used,
                baseline_buckets: st.n,
                baseline_range: st.range,
            });
        }
        i = j.max(i + 1);
    }
    runs
}

/// True when the newest bucket is flagged but not yet confirmed by a
/// second one -- "something may be starting", reported as such.
pub fn tail_unconfirmed(
    values: &[Option<f64>],
    kind: Kind,
    baseline: Baseline,
    cfg: &ChangepointCfg,
) -> bool {
    let zs = zscores(values, kind, baseline, cfg);
    let n = zs.len();
    if n == 0 {
        return false;
    }
    let flagged = |i: usize| {
        zs.get(i)
            .and_then(|z| z.as_ref())
            .is_some_and(|(z, _)| z.abs() >= cfg.z_threshold)
    };
    if !flagged(n - 1) {
        return false;
    }
    let mut len = 1;
    while n > len && flagged(n - 1 - len) {
        len += 1;
    }
    len < cfg.consecutive_buckets
}

/// A bucketed series with its identity. `labels` are Prometheus-style and
/// sorted, so the key is canonical.
#[derive(Clone, Debug)]
pub struct Series {
    pub metric: &'static str,
    pub labels: BTreeMap<String, String>,
    pub kind: Kind,
    pub values: Vec<Option<f64>>,
}

impl Series {
    pub fn key(&self) -> String {
        let l: Vec<String> = self
            .labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{v}\""))
            .collect();
        format!("{}{{{}}}", self.metric, l.join(","))
    }
}

/// Per-bucket accumulator for one label set.
#[derive(Default, Clone, Copy, Debug)]
pub struct Acc {
    pub requests: u64,
    pub errors: u64,
    pub lat_sum: f64,
    pub lat_n: u64,
}

pub const METRICS: [&str; 4] = [
    "error_rate",
    "errors_total",
    "requests_total",
    "latency_ms_mean",
];

/// Turn per-bucket accumulators (index -> Acc, `n` buckets, buckets before
/// `history_from` undefined) into the four series for one label set.
pub fn series_from(labels: BTreeMap<String, String>, accs: &[Option<Acc>]) -> Vec<Series> {
    let mk = |metric: &'static str, kind: Kind, f: &dyn Fn(&Acc) -> Option<f64>| Series {
        metric,
        labels: labels.clone(),
        kind,
        values: accs.iter().map(|a| a.as_ref().and_then(f)).collect(),
    };
    vec![
        mk("error_rate", Kind::Rate, &|a| {
            if a.requests > 0 {
                Some(a.errors as f64 / a.requests as f64)
            } else {
                None
            }
        }),
        mk("errors_total", Kind::Count, &|a| Some(a.errors as f64)),
        mk("requests_total", Kind::Count, &|a| Some(a.requests as f64)),
        mk("latency_ms_mean", Kind::Latency, &|a| {
            if a.lat_n > 0 {
                Some(a.lat_sum / a.lat_n as f64)
            } else {
                None
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChangepointCfg {
        ChangepointCfg {
            bucket_secs: 10,
            z_threshold: 4.0,
            consecutive_buckets: 2,
            baseline_secs: 900,
            guard_secs: 30,
            min_baseline_buckets: 6,
            sigma_floor_count: 1.0,
            sigma_floor_rate: 0.01,
            sigma_floor_latency_frac: 0.05,
            sigma_floor_latency_ms: 2.0,
        }
    }

    fn step(pre: usize, pre_val: f64, post: usize, post_val: f64) -> Vec<Option<f64>> {
        let mut v: Vec<Option<f64>> = (0..pre)
            .map(|i| Some(pre_val + 0.002 * ((i % 3) as f64 - 1.0)))
            .collect();
        v.extend((0..post).map(|i| Some(post_val + 0.01 * ((i % 2) as f64))));
        v
    }

    #[test]
    fn a_step_in_error_rate_is_reported_once_at_its_first_bucket() {
        let v = step(60, 0.01, 8, 0.20);
        let runs = detect(&v, Kind::Rate, Baseline::Rolling, &cfg());
        assert_eq!(runs.len(), 1, "{runs:?}");
        let r = &runs[0];
        assert_eq!((r.start, r.direction), (60, Direction::Up));
        assert!(r.z_first > 4.0 && r.len >= 2, "{r:?}");
        assert!((r.baseline_mean - 0.01).abs() < 0.002);
    }

    #[test]
    fn a_single_spike_is_not_a_changepoint() {
        let mut v = step(60, 0.01, 0, 0.0);
        v.push(Some(0.5));
        v.extend(step(10, 0.01, 0, 0.0));
        assert!(detect(&v, Kind::Rate, Baseline::Rolling, &cfg()).is_empty());
    }

    #[test]
    fn a_flat_series_never_fires_because_sigma_is_floored() {
        // exactly constant baseline, then a wobble of one unit: without a
        // floor sigma = 0 and z = infinity
        let mut v: Vec<Option<f64>> = vec![Some(100.0); 60];
        v.extend([101.0, 99.0, 102.0, 100.0].map(Some));
        assert!(detect(&v, Kind::Count, Baseline::Rolling, &cfg()).is_empty());
        // but a real traffic step (100 -> 0, sigma floored to sqrt(100)) does fire
        v.extend([0.0, 0.0, 0.0].map(Some));
        let runs = detect(&v, Kind::Count, Baseline::Rolling, &cfg());
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].start, runs[0].direction), (64, Direction::Down));
    }

    #[test]
    fn too_little_baseline_is_undetermined_not_flagged() {
        // 4 buckets of history then a huge step: fewer than 6 baseline buckets
        let v = step(4, 0.01, 4, 0.9);
        assert!(detect(&v, Kind::Rate, Baseline::Rolling, &cfg()).is_empty());
        // with 9 buckets (the fast timeline: 90 s) it fires -- guard 30 s leaves 6
        let v = step(9, 0.01, 4, 0.9);
        assert_eq!(detect(&v, Kind::Rate, Baseline::Rolling, &cfg()).len(), 1);
    }

    #[test]
    fn the_guard_keeps_the_change_out_of_its_own_baseline_while_confirming() {
        let v = step(60, 0.01, 3, 0.20);
        let runs = detect(&v, Kind::Rate, Baseline::Rolling, &cfg());
        assert_eq!(runs.len(), 1);
        // all three post buckets flagged: bucket 62's baseline ends at 59 (guard 3), still clean
        assert_eq!(runs[0].len, 3);
        assert_eq!(runs[0].baseline_range, (0, 57));
    }

    #[test]
    fn an_explicit_baseline_detects_recovery_as_a_downward_step() {
        // incident at 0.2 for buckets 60..72, then back to 0.0
        let mut v = step(60, 0.01, 12, 0.20);
        v.extend(vec![Some(0.0); 6]);
        let runs = detect(&v, Kind::Rate, Baseline::Explicit(60, 72), &cfg());
        // relative to the incident level the recovery is a confirmed downward
        // step at bucket 72 (the pre-incident buckets are "down" too; the
        // tool's window is what restricts reporting to after the action)
        let rec: Vec<_> = runs.iter().filter(|r| r.start == 72).collect();
        assert_eq!(rec.len(), 1, "{runs:?}");
        assert_eq!(rec[0].direction, Direction::Down);
        assert!(
            runs.iter().all(|r| r.start < 60 || r.start == 72),
            "{runs:?}"
        );
    }

    #[test]
    fn a_lone_flagged_newest_bucket_is_reported_as_unconfirmed() {
        let v = step(60, 0.01, 1, 0.2);
        assert!(detect(&v, Kind::Rate, Baseline::Rolling, &cfg()).is_empty());
        assert!(tail_unconfirmed(&v, Kind::Rate, Baseline::Rolling, &cfg()));
        let v = step(60, 0.01, 2, 0.2);
        assert!(!tail_unconfirmed(&v, Kind::Rate, Baseline::Rolling, &cfg()));
    }

    #[test]
    fn undefined_buckets_are_skipped_not_treated_as_zero() {
        // rate series with no requests in some buckets -> None; a None bucket
        // inside a run breaks it (no value, no vote) and never counts as a drop
        let mut v = step(60, 0.01, 2, 0.2);
        v.push(None);
        v.extend(step(2, 0.2, 0, 0.0));
        let runs = detect(&v, Kind::Rate, Baseline::Rolling, &cfg());
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|r| r.direction == Direction::Up));
    }

    #[test]
    fn series_keys_are_canonical_prometheus_style() {
        let mut l = BTreeMap::new();
        l.insert("service".to_string(), "orders".to_string());
        l.insert("route".to_string(), "/orders".to_string());
        let s = series_from(
            l,
            &[
                Some(Acc {
                    requests: 10,
                    errors: 2,
                    lat_sum: 100.0,
                    lat_n: 10,
                }),
                None,
            ],
        );
        assert_eq!(
            s[0].key(),
            "error_rate{route=\"/orders\",service=\"orders\"}"
        );
        assert_eq!(s[0].values, vec![Some(0.2), None]);
        assert_eq!(s[1].values, vec![Some(2.0), None]);
        assert_eq!(s[3].values, vec![Some(10.0), None]);
    }
}
