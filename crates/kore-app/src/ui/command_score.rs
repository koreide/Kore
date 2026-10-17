//! Port of `command-score` as bundled in cmdk 1.x: the fuzzy score cmdk uses
//! to filter and sort palette items (0 = hidden).

const SCORE_CONTINUE_MATCH: f64 = 1.0;
const SCORE_SPACE_WORD_JUMP: f64 = 0.9;
const SCORE_NON_SPACE_WORD_JUMP: f64 = 0.8;
const SCORE_CHARACTER_JUMP: f64 = 0.17;
const SCORE_TRANSPOSITION: f64 = 0.1;
const PENALTY_SKIPPED: f64 = 0.999;
const PENALTY_CASE_MISMATCH: f64 = 0.9999;
const PENALTY_NOT_COMPLETE: f64 = 0.99;

fn is_gap(c: char) -> bool {
    matches!(c, '\\' | '/' | '_' | '+' | '.' | '#' | '"' | '@' | '[' | '(' | '{' | '&')
}

fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '-'
}

fn format_input(s: &[char]) -> Vec<char> {
    s.iter().flat_map(|c| c.to_lowercase()).map(|c| if is_space(c) { ' ' } else { c }).collect()
}

/// Score `abbreviation` (the query) against `string` (the item value).
pub fn command_score(string: &str, abbreviation: &str) -> f64 {
    let s: Vec<char> = string.chars().collect();
    let a: Vec<char> = abbreviation.chars().collect();
    let ls = format_input(&s);
    let la = format_input(&a);
    // Lowercasing can change length for exotic chars; fall back to originals.
    if ls.len() != s.len() || la.len() != a.len() {
        return if string.to_lowercase().contains(&abbreviation.to_lowercase()) { 0.5 } else { 0.0 };
    }
    let mut memo = std::collections::HashMap::new();
    inner(&s, &a, &ls, &la, 0, 0, &mut memo)
}

fn index_of(hay: &[char], c: char, from: usize) -> Option<usize> {
    hay.iter().skip(from).position(|&x| x == c).map(|i| i + from)
}

fn inner(
    s: &[char],
    a: &[char],
    ls: &[char],
    la: &[char],
    si: usize,
    ai: usize,
    memo: &mut std::collections::HashMap<(usize, usize), f64>,
) -> f64 {
    if ai == a.len() {
        return if si == s.len() { SCORE_CONTINUE_MATCH } else { PENALTY_NOT_COMPLETE };
    }
    if let Some(v) = memo.get(&(si, ai)) {
        return *v;
    }
    let ac = la[ai];
    let mut index = index_of(ls, ac, si);
    let mut high = 0.0f64;
    while let Some(ix) = index {
        let mut score = inner(s, a, ls, la, ix + 1, ai + 1, memo);
        if score > high {
            if ix == si {
                score *= SCORE_CONTINUE_MATCH;
            } else if ix > 0 && is_gap(s[ix - 1]) {
                score *= SCORE_NON_SPACE_WORD_JUMP;
                let breaks = s[si..ix.saturating_sub(1).max(si)].iter().filter(|c| is_gap(**c)).count();
                if breaks > 0 && si > 0 {
                    score *= PENALTY_SKIPPED.powi(breaks as i32);
                }
            } else if ix > 0 && is_space(s[ix - 1]) {
                score *= SCORE_SPACE_WORD_JUMP;
                let breaks = s[si..ix.saturating_sub(1).max(si)].iter().filter(|c| is_space(**c)).count();
                if breaks > 0 && si > 0 {
                    score *= PENALTY_SKIPPED.powi(breaks as i32);
                }
            } else {
                score *= SCORE_CHARACTER_JUMP;
                if si > 0 {
                    score *= PENALTY_SKIPPED.powi((ix - si) as i32);
                }
            }
            if s[ix] != a[ai] {
                score *= PENALTY_CASE_MISMATCH;
            }
        }
        let next_a = la.get(ai + 1).copied();
        let prev_s = if ix > 0 { ls.get(ix - 1).copied() } else { None };
        if (score < SCORE_TRANSPOSITION && prev_s.is_some() && prev_s == next_a)
            || (next_a == Some(la[ai]) && prev_s != Some(la[ai]))
        {
            let t = inner(s, a, ls, la, ix + 1, ai + 2, memo);
            if t * SCORE_TRANSPOSITION > score {
                score = t * SCORE_TRANSPOSITION;
            }
        }
        if score > high {
            high = score;
        }
        index = index_of(ls, ac, ix + 1);
    }
    memo.insert((si, ai), high);
    high
}

#[cfg(test)]
mod tests {
    use super::command_score;

    #[test]
    fn basic_matching() {
        assert_eq!(command_score("view-Dashboard", "zzzzqqq"), 0.0);
        assert!(command_score("view-Dashboard", "Dashboard") > 0.0);
        assert!(command_score("nav-Secrets", "Secrets") > command_score("ctx-kind-kore-harness", "Secrets"));
        // Exact prefix beats scattered matches.
        assert!(command_score("view-Dashboard", "Dashboard") > command_score("nav-Deployments", "Dashboard"));
        assert_eq!(command_score("nav-Deployments", ":deployments"), 0.0);
        assert!(command_score("filter-:deployments", ":deployments") > 0.0);
        assert!(command_score("search-pods-multi-container", "multi-co") > 0.0);
    }

    #[test]
    fn full_match_is_one() {
        assert_eq!(command_score("abc", "abc"), 1.0);
        assert!((command_score("abcd", "abc") - 0.99).abs() < 1e-9);
    }
}
