//! Lightweight matching used for favourites and the app picker.

/// Scores how well `query` matches `text`. `None` = no match; higher is better.
/// Every whitespace-separated query word must match as a prefix of a word,
/// a substring, or (weakest) an in-order subsequence.
pub fn score(query: &str, text: &str) -> Option<u32> {
    let text_l = text.to_lowercase();
    let mut total = 0;
    for term in query.split_whitespace() {
        let term = term.to_lowercase();
        total += term_score(&term, &text_l)?;
    }
    Some(total)
}

fn term_score(term: &str, text: &str) -> Option<u32> {
    if text.starts_with(term) {
        return Some(100);
    }
    if text
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(term))
    {
        return Some(80);
    }
    if text.contains(term) {
        return Some(50);
    }
    let mut chars = text.chars();
    term.chars().all(|c| chars.any(|t| t == c)).then_some(10)
}

/// Best score over several fields (name, then keywords with a penalty).
pub fn score_fields(query: &str, name: &str, extra: &[&str]) -> Option<u32> {
    let best_extra = extra.iter().filter_map(|e| score(query, e)).max().map(|s| s / 2);
    match (score(query, name), best_extra) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking() {
        assert!(score("fire", "Firefox").unwrap() > score("fox", "Firefox").unwrap());
        assert!(score("code", "Visual Studio Code").unwrap() > score("vsc", "Visual Studio Code").unwrap());
        assert_eq!(score("zzz", "Firefox"), None);
        assert!(score("vis code", "Visual Studio Code").is_some());
        assert!(score("", "anything").is_some());
        assert_eq!(score_fields("browser", "Firefox", &["Web Browser"]), Some(40));
    }
}
