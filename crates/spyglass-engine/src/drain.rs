//! Simplified Drain (He et al., *Drain: An Online Log Parsing Approach with
//! Fixed Depth Tree*, ICWS 2017).
//!
//! Messages are masked first (numbers, ids, hex, timestamps, currency codes
//! become `<*>` -- `spyglass_core::mask`), then tokenised on whitespace and
//! routed through a fixed-depth tree: token count, then the first
//! `depth - 2` tokens (a token containing a digit or already `<*>` routes to
//! the wildcard branch). At the leaf, the message is compared to each
//! cluster's template by the fraction of positions whose tokens are equal
//! (`<*>` never counts as a match); the best cluster at or above
//! `similarity_threshold` absorbs the message and every position that
//! differs becomes `<*>`. Otherwise a new cluster is created.
//!
//! Cluster ids are stable for the life of the store even as a cluster's
//! template acquires more wildcards; the *pattern* is a property of the
//! cluster at read time. Same events in the same order -> same tree -> same
//! ids (ADR-004).

use std::collections::HashMap;

use spyglass_core::DrainCfg;

const WILDCARD: &str = "<*>";

#[derive(Debug, Clone)]
pub struct Cluster {
    pub id: u64,
    pub tokens: Vec<String>,
    pub size: u64,
}

impl Cluster {
    pub fn template(&self) -> String {
        self.tokens.join(" ")
    }
}

#[derive(Default, Debug)]
struct Node {
    children: HashMap<String, Node>,
    clusters: Vec<u64>,
}

pub struct Drain {
    cfg: DrainCfg,
    root: Node,
    clusters: HashMap<u64, Cluster>,
    next_id: u64,
}

fn has_digit(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_digit())
}

/// Fraction of positions whose tokens are identical. `<*>` in the template
/// never counts, so a heavily wildcarded template does not attract everything.
fn seq_dist(template: &[String], tokens: &[String]) -> f64 {
    if template.is_empty() {
        return 0.0;
    }
    let same = template
        .iter()
        .zip(tokens)
        .filter(|(a, b)| a.as_str() != WILDCARD && a == b)
        .count();
    same as f64 / template.len() as f64
}

impl Drain {
    pub fn new(cfg: DrainCfg) -> Self {
        Self {
            cfg,
            root: Node::default(),
            clusters: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.clusters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clusters.is_empty()
    }

    pub fn cluster(&self, id: u64) -> Option<&Cluster> {
        self.clusters.get(&id)
    }

    /// Route a tokenised (already masked) message to a cluster. Returns
    /// `(cluster id, created)`.
    pub fn insert(&mut self, tokens: Vec<String>) -> (u64, bool) {
        self.insert_keyed("", tokens)
    }

    /// Like `insert`, with an extra routing key ahead of token count -- the
    /// log level, in the store's case. The key partitions the tree so that
    /// `INFO request completed` and `ERROR request failed` can never meet at
    /// a leaf, while similarity is still measured over message tokens only.
    pub fn insert_keyed(&mut self, key: &str, tokens: Vec<String>) -> (u64, bool) {
        // Level 0: caller's key. Level 1: token count. Levels 2..depth:
        // leading tokens (wildcarded when they look variable). Leaves hold
        // cluster ids.
        let mut node = &mut self.root;
        if !key.is_empty() {
            node = node.children.entry(format!("key:{key}")).or_default();
        }
        let len_key = tokens.len().to_string();
        node = node.children.entry(len_key).or_default();
        let lead = self.cfg.depth.saturating_sub(2).min(tokens.len());
        for tok in tokens.iter().take(lead) {
            let key = if tok == WILDCARD || has_digit(tok) {
                WILDCARD.to_string()
            } else {
                tok.clone()
            };
            if !node.children.contains_key(&key) && node.children.len() >= self.cfg.max_children {
                node = node.children.entry(WILDCARD.to_string()).or_default();
            } else {
                node = node.children.entry(key).or_default();
            }
        }

        // Best matching cluster at the leaf.
        let mut best: Option<(u64, f64)> = None;
        for &cid in &node.clusters {
            let sim = seq_dist(&self.clusters[&cid].tokens, &tokens);
            if best.is_none_or(|(_, b)| sim > b) {
                best = Some((cid, sim));
            }
        }
        if let Some((cid, sim)) = best {
            // Two-token messages sharing one token ("request completed" /
            // "request captured") hit 0.5 exactly; a merge on one matching
            // token is a coincidence, not a template. Require at least two
            // agreeing positions unless the message is a single token.
            let agreeing = self.clusters[&cid]
                .tokens
                .iter()
                .zip(&tokens)
                .filter(|(a, b)| a.as_str() != WILDCARD && a == b)
                .count();
            if sim >= self.cfg.similarity_threshold && (agreeing >= 2 || tokens.len() == 1) {
                let c = self.clusters.get_mut(&cid).expect("cluster exists");
                for (t, tok) in c.tokens.iter_mut().zip(&tokens) {
                    if t != tok {
                        *t = WILDCARD.to_string();
                    }
                }
                c.size += 1;
                return (cid, false);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        node.clusters.push(id);
        self.clusters.insert(
            id,
            Cluster {
                id,
                tokens,
                size: 1,
            },
        );
        (id, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> DrainCfg {
        DrainCfg {
            depth: 3,
            similarity_threshold: 0.5,
            max_children: 100,
        }
    }
    fn toks(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn merges_lines_that_differ_in_one_variable_token() {
        let mut d = Drain::new(cfg());
        let (a, created_a) = d.insert(toks("user alice logged in"));
        let (b, created_b) = d.insert(toks("user bob logged in"));
        assert!(created_a && !created_b);
        assert_eq!(a, b);
        assert_eq!(d.cluster(a).unwrap().template(), "user <*> logged in");
        assert_eq!(d.cluster(a).unwrap().size, 2);
    }

    #[test]
    fn different_token_counts_never_merge() {
        let mut d = Drain::new(cfg());
        let (a, _) = d.insert(toks("request completed"));
        let (b, _) = d.insert(toks("request completed with warnings"));
        assert_ne!(a, b);
    }

    #[test]
    fn below_threshold_creates_a_new_cluster() {
        let mut d = Drain::new(cfg());
        let (a, _) = d.insert(toks("payments charge failed with HTTP <*>"));
        let (b, _) = d.insert(toks("payments charge cached for request <*>"));
        assert_ne!(
            a, b,
            "2 of 6 positions match (0.33) -- must not merge at 0.5"
        );
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn two_token_messages_sharing_one_token_do_not_merge() {
        let mut d = Drain::new(cfg());
        let (a, _) = d.insert(toks("request completed"));
        let (b, _) = d.insert(toks("request captured"));
        assert_ne!(a, b);
        let (c, _) = d.insert(toks("request completed"));
        assert_eq!(a, c);
    }

    #[test]
    fn the_routing_key_partitions_without_counting_as_a_match() {
        let mut d = Drain::new(cfg());
        let (a, _) = d.insert_keyed("INFO", toks("request completed"));
        let (b, _) = d.insert_keyed("INFO", toks("request captured"));
        let (c, _) = d.insert_keyed("ERROR", toks("request failed"));
        let (a2, _) = d.insert_keyed("INFO", toks("request completed"));
        assert_ne!(a, b, "one agreeing token of two is not a template");
        assert_ne!(a, c, "different key, different subtree");
        assert_eq!(a, a2);
        assert_eq!(d.cluster(a).unwrap().template(), "request completed");
    }

    #[test]
    fn wildcard_positions_do_not_attract_everything() {
        let mut d = Drain::new(cfg());
        let (a, _) = d.insert(toks("<*> <*> <*> done"));
        let (b, _) = d.insert(toks("<*> <*> <*> failed"));
        // only 0 of 4 non-wildcard positions match ("done" vs "failed") -> new cluster
        assert_ne!(a, b);
    }

    #[test]
    fn ids_are_stable_across_merges_and_order_is_deterministic() {
        let lines = [
            "user alice logged in",
            "user bob logged in",
            "job <*> finished",
            "job <*> failed",
            "user carol logged in",
        ];
        let run = || {
            let mut d = Drain::new(cfg());
            lines
                .iter()
                .map(|l| d.insert(toks(l)).0)
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
        let mut d = Drain::new(cfg());
        let ids: Vec<u64> = lines.iter().map(|l| d.insert(toks(l)).0).collect();
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[0], ids[4]);
        // "job <*> finished" vs "job <*> failed": 1 of 3 non-wildcard-comparable positions... seq_dist = 1/3 < 0.5 -> distinct
        assert_ne!(ids[2], ids[3]);
    }
}
