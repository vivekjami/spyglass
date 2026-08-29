//! Ingestion: tail the log files and the deploy journal, scrape /metrics.
//!
//! Files are tailed by byte offset; only complete lines (ending in '\n') are
//! consumed, so a half-written line is read next tick, never as garbage. A
//! file that shrinks means the stack was reset -- the store is cleared and
//! the epoch bumped, so evidence from a previous incident never leaks into
//! this one. Every consumed raw line is also appended to a per-minute segment
//! file (README C2), the durable copy that later phases index.

use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use chrono::Utc;
use spyglass_core::{DeployEvent, Event};

use crate::Engine;

const RAW_CAP: usize = 4096;

struct Cursor {
    offset: u64,
    lineno: u64,
}

/// Read complete new lines from `path` starting at the cursor. Returns None
/// if the file shrank (caller resets).
fn read_new_lines(path: &Path, cur: &mut Cursor) -> Option<Vec<(u64, String)>> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() < cur.offset {
        return None;
    }
    if meta.len() == cur.offset {
        return Some(vec![]);
    }
    let mut f = fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(cur.offset)).ok()?;
    let mut r = BufReader::new(f);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = r.read_until(b'\n', &mut buf).ok()?;
        if n == 0 {
            break;
        }
        if buf.last() != Some(&b'\n') {
            break; // partial line: leave it for the next tick
        }
        cur.offset += n as u64;
        cur.lineno += 1;
        let line = String::from_utf8_lossy(&buf[..n - 1]).into_owned();
        if !line.trim().is_empty() {
            out.push((cur.lineno, line));
        }
    }
    Some(out)
}

fn append_segment(dir: &Path, instance: &str, ts: chrono::DateTime<Utc>, raw: &str) {
    let d = dir.join(instance);
    let _ = fs::create_dir_all(&d);
    let p = d.join(format!("{}.jsonl", ts.format("%Y%m%dT%H%M")));
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{raw}");
    }
}

fn log_tailer(engine: Arc<Engine>) {
    let cfg = engine.cfg.clone();
    let mut cursors: HashMap<PathBuf, Cursor> = HashMap::new();
    loop {
        let mut files: Vec<PathBuf> = fs::read_dir(&cfg.paths.log_dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect())
            .unwrap_or_default();
        files.sort();
        let mut reset = false;
        for path in &files {
            let instance = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            let cur = cursors.entry(path.clone()).or_insert(Cursor { offset: 0, lineno: 0 });
            match read_new_lines(path, cur) {
                None => {
                    tracing::warn!("{} shrank; stack reset -> clearing store (new epoch)", path.display());
                    reset = true;
                    break;
                }
                Some(lines) if lines.is_empty() => {}
                Some(lines) => {
                    let mut parsed = Vec::with_capacity(lines.len());
                    let mut bad = 0u64;
                    let mut replayed = 0u64;
                    for (lineno, raw) in lines {
                        match Event::parse(&raw, &instance, format!("{instance}:{lineno}"), RAW_CAP) {
                            // The engine's own replay traffic (README C9) is
                            // tagged by request id; it is an experiment, not
                            // evidence, and must not move a count, a rate, or
                            // a watermark. Counted, never stored.
                            Some(e) if e.req_id.as_deref().is_some_and(|r| r.starts_with(crate::replay::REQ_ID_PREFIX)) => replayed += 1,
                            Some(e) => {
                                append_segment(&cfg.paths.segment_dir, &instance, e.ts, &raw);
                                parsed.push(e);
                            }
                            None => bad += 1,
                        }
                    }
                    let mut s = engine.store.write().expect("store lock");
                    for e in parsed {
                        s.append(e);
                    }
                    s.malformed += bad;
                    s.replay_lines_excluded += replayed;
                }
            }
        }
        if reset {
            engine.store.write().expect("store lock").reset();
            engine.caught_up.store(false, std::sync::atomic::Ordering::Relaxed);
            cursors.clear();
            continue;
        }
        // Every file has been read to its end once: from here on, lines are live.
        engine.caught_up.store(true, std::sync::atomic::Ordering::Relaxed);
        thread::sleep(Duration::from_millis(cfg.ingest.poll_ms));
    }
}

fn journal_tailer(engine: Arc<Engine>) {
    let path = engine.cfg.paths.deploy_dir.join("journal.jsonl");
    let mut cur = Cursor { offset: 0, lineno: 0 };
    loop {
        match read_new_lines(&path, &mut cur) {
            None => {
                // `deployer init --reset` rotates the journal: start over.
                let mut s = engine.store.write().expect("store lock");
                s.deploys.clear();
                cur = Cursor { offset: 0, lineno: 0 };
            }
            Some(lines) => {
                if !lines.is_empty() {
                    let mut s = engine.store.write().expect("store lock");
                    for (_, raw) in lines {
                        if let Ok(d) = serde_json::from_str::<DeployEvent>(&raw) {
                            s.watermarks.entry("journal".into()).and_modify(|w| *w = (*w).max(d.ts)).or_insert(d.ts);
                            s.deploys.push(d);
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(engine.cfg.ingest.poll_ms));
    }
}

const KEEP: &[&str] = &["requests_total", "errors_total", "upstream_requests_total", "latency_ms_count", "latency_ms_sum"];

async fn metrics_scraper(engine: Arc<Engine>) {
    let cfg = engine.cfg.clone();
    let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().expect("http client");
    let targets: Vec<(String, String)> = cfg
        .services
        .iter()
        .filter_map(|s| cfg.metrics_url(s).map(|u| (s.name.clone(), u)))
        .collect();
    loop {
        let now = Utc::now();
        for (instance, url) in &targets {
            let Ok(resp) = client.get(url).send().await else { continue };
            let Ok(text) = resp.text().await else { continue };
            let mut s = engine.store.write().expect("store lock");
            for line in text.lines() {
                if line.starts_with('#') {
                    continue;
                }
                let Some((lhs, val)) = line.rsplit_once(' ') else { continue };
                let name = lhs.split('{').next().unwrap_or("");
                if !KEEP.contains(&name) {
                    continue;
                }
                let Ok(v) = val.parse::<f64>() else { continue };
                let key = format!("{instance}|{lhs}");
                let ring = s.metrics.entry(key).or_default();
                ring.push_back((now, v));
                while ring.len() > cfg.ingest.metrics_ring {
                    ring.pop_front();
                }
            }
            s.watermarks.insert("metrics".into(), now);
        }
        tokio::time::sleep(Duration::from_secs(cfg.ingest.metrics_scrape_secs)).await;
    }
}

pub fn spawn(engine: Arc<Engine>) {
    let e1 = engine.clone();
    thread::Builder::new().name("log-tailer".into()).spawn(move || log_tailer(e1)).expect("spawn tailer");
    let e2 = engine.clone();
    thread::Builder::new().name("journal-tailer".into()).spawn(move || journal_tailer(e2)).expect("spawn journal");
    tokio::spawn(metrics_scraper(engine));
}
