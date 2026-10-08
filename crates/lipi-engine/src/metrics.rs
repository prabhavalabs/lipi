//! Accuracy metrics: character and word error rates, and an order-insensitive agreement score.

use std::collections::HashMap;

/// Collapse whitespace and normalise for comparison.
pub fn canonical(s: &str) -> String {
    lipi_script::normalize(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Levenshtein distance between two sequences.
pub fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(x != y);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Character error rate of `hypothesis` against `reference` (after [`canonical`]).
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    let r: Vec<char> = canonical(reference).chars().collect();
    let h: Vec<char> = canonical(hypothesis).chars().collect();
    levenshtein(&r, &h) as f64 / r.len().max(1) as f64
}

/// Word error rate of `hypothesis` against `reference` (after [`canonical`]).
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let r = canonical(reference);
    let h = canonical(hypothesis);
    let rw: Vec<&str> = r.split(' ').filter(|w| !w.is_empty()).collect();
    let hw: Vec<&str> = h.split(' ').filter(|w| !w.is_empty()).collect();
    levenshtein(&rw, &hw) as f64 / rw.len().max(1) as f64
}

fn bigrams(s: &str) -> HashMap<(char, char), usize> {
    let cs: Vec<char> = canonical(s).chars().filter(|c| !c.is_whitespace()).collect();
    let mut m = HashMap::new();
    for w in cs.windows(2) {
        *m.entry((w[0], w[1])).or_insert(0) += 1;
    }
    m
}

/// Order-insensitive agreement between two texts: Dice coefficient over character bigrams.
/// Robust to differences in reading order between a text layer and OCR; `1.0` is identical.
pub fn agreement(a: &str, b: &str) -> f64 {
    let (x, y) = (bigrams(a), bigrams(b));
    let (nx, ny): (usize, usize) = (x.values().sum(), y.values().sum());
    if nx + ny == 0 {
        return 1.0;
    }
    let common: usize = x.iter().map(|(k, v)| (*v).min(*y.get(k).unwrap_or(&0))).sum();
    2.0 * common as f64 / (nx + ny) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_rates() {
        assert_eq!(cer("abc", "abc"), 0.0);
        assert!((cer("abcd", "abxd") - 0.25).abs() < 1e-9);
        assert!((wer("the cat sat", "the bat sat") - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(cer("ශ්‍රී  ලංකා", "ශ්‍රී ලංකා"), 0.0);
    }

    #[test]
    fn agreement_ignores_order() {
        assert!((agreement("alpha beta", "beta alpha") - 1.0).abs() < 0.2);
        assert!(agreement("ශ්‍රී ලංකා ප්‍රජාතාන්ත්‍රික", "ශී ලංකා පජාතාන්තික") < 0.9);
        assert!(agreement("abc", "xyz") < 0.01);
    }
}
