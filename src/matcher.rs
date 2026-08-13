/// Subsequence fuzzy match. Returns None if `query` is not a subsequence of
/// `target_lower`; otherwise a score where higher is better.
/// Both inputs must already be lowercase.
pub fn score(query: &str, target_lower: &str) -> Option<i32> {
    let mut score = 0i32;
    let mut it = target_lower.char_indices();
    let mut prev_end: Option<usize> = None;
    let mut first_match: Option<usize> = None;

    for qc in query.chars() {
        let (i, tc) = it.by_ref().find(|&(_, tc)| tc == qc)?;
        if i == 0 {
            score += 12; // prefix of the whole name
        } else if prev_end == Some(i) {
            score += 8; // consecutive run
        } else if matches!(
            target_lower[..i].chars().last(),
            Some(' ') | Some('-') | Some('_') | Some('.') | Some('(')
        ) {
            score += 10; // word boundary
        }
        if first_match.is_none() {
            first_match = Some(i);
        }
        prev_end = Some(i + tc.len_utf8());
    }

    // Earlier first hit and shorter names win ties.
    score += 20 - first_match.unwrap_or(0).min(20) as i32;
    score -= target_lower.len() as i32 / 8;
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn prefix_beats_scattered() {
        let claude = score("cla", "claude").unwrap();
        let scattered = score("cla", "calculator").unwrap();
        assert!(claude > scattered, "claude={claude} calculator={scattered}");
    }

    #[test]
    fn word_boundary_bonus() {
        let boundary = score("code", "visual studio code").unwrap();
        assert!(boundary > 0);
    }

    #[test]
    fn non_match_is_none() {
        assert!(score("xyz", "claude").is_none());
    }
}
