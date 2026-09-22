//! Ranked, case-insensitive subsequence matching for navigation pickers.

pub(super) fn ranked<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    query: &str,
) -> Vec<usize> {
    let query = query.to_lowercase();
    let tokens: Vec<_> = query.split_whitespace().collect();
    let mut matches = Vec::new();
    for (index, (name, hint)) in entries.into_iter().enumerate() {
        let name = name.to_lowercase();
        let hint = hint.to_lowercase();
        let score = tokens.iter().try_fold(0usize, |total, token| {
            let primary = score(&name, token);
            let secondary = score(&hint, token).map(|score| score + 10_000);
            match (primary, secondary) {
                (Some(a), Some(b)) => Some(total + a.min(b)),
                (Some(a), None) | (None, Some(a)) => Some(total + a),
                (None, None) => None,
            }
        });
        if let Some(score) = score {
            matches.push((score, index));
        }
    }
    matches.sort_unstable();
    matches.into_iter().map(|(_, index)| index).collect()
}

fn score(text: &str, query: &str) -> Option<usize> {
    if text == query {
        return Some(0);
    }
    if text.starts_with(query) {
        return Some(100);
    }
    if let Some(start) = text.find(query) {
        return Some(300 + start.min(500));
    }
    let mut needle = query.chars();
    let mut next = needle.next()?;
    let mut first = None;
    let mut matched = 0;
    for (index, ch) in text.chars().enumerate() {
        if ch == next {
            let start = *first.get_or_insert(index);
            matched += 1;
            match needle.next() {
                Some(ch) => next = ch,
                None => return Some(1000 + start + index + 1 - matched),
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(entries: &[(String, String)], query: &str) -> Vec<usize> {
        ranked(
            entries
                .iter()
                .map(|(name, hint)| (name.as_str(), hint.as_str())),
            query,
        )
    }

    #[test]
    fn exact_prefix_and_substring_rank_before_gapped_names_and_path_matches() {
        let entries = [
            ("read-more-examples".into(), "".into()),
            ("RME notes".into(), "".into()),
            ("RME".into(), "".into()),
            ("other".into(), "/RME/other.md".into()),
            ("the RME plan".into(), "".into()),
        ];
        assert_eq!(matches(&entries, "rme"), vec![2, 1, 4, 0, 3]);
        assert_eq!(matches(&entries, ""), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn unicode_and_multiple_tokens_can_match_title_and_path_without_reordering_ties() {
        let entries = [
            ("Résumé 界面".into(), "/drafts/one.md".into()),
            ("Résumé 界面".into(), "/archive/one.md".into()),
            ("Résumé 界面".into(), "/drafts/two.md".into()),
        ];
        assert_eq!(matches(&entries, "RÉM 界 draft"), vec![0, 2]);
        assert!(matches(&entries, "面界").is_empty());
        assert!(matches(&entries, "missing").is_empty());
    }
}
