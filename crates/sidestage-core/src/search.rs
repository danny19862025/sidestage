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
    let best_extra = extra
        .iter()
        .filter_map(|e| score(query, e))
        .max()
        .map(|s| s / 2);
    match (score(query, name), best_extra) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// One result of the combined favourites + minimized-windows search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchHit {
    Favourite(String),
    Window(String),
}

/// Ranks favourites `(id, name, section)` and minimized windows
/// `(id, title, app name)` together. Higher scores first; on a tie
/// favourites come before windows, then the input order is kept.
pub fn search_all<'a>(
    query: &str,
    favourites: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
    windows: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> Vec<SearchHit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u32, u8, usize, SearchHit)> = Vec::new();
    for (i, (id, name, section)) in favourites.into_iter().enumerate() {
        if let Some(sc) = score_fields(query, name, &[section]) {
            scored.push((sc, 0, i, SearchHit::Favourite(id.to_string())));
        }
    }
    for (i, (id, title, app)) in windows.into_iter().enumerate() {
        // Words may match across title and app ("firefox github").
        let both = format!("{title} {app}");
        let sc = [
            score(query, title),
            score(query, app),
            score(query, &both).map(|s| s.saturating_sub(5)),
        ]
        .into_iter()
        .flatten()
        .max();
        if let Some(sc) = sc {
            scored.push((sc, 1, i, SearchHit::Window(id.to_string())));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    scored.into_iter().map(|t| t.3).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_search() {
        let favs = [("f1", "Firefox", "Everyday"), ("f2", "Terminal", "Coding")];
        let wins = [
            ("w1", "GitHub - sidestage", "Firefox"),
            ("w2", "Inbox", "Thunderbird"),
            ("w3", "Rust docs", "Firefox"),
        ];
        let ids = |v: Vec<SearchHit>| -> Vec<String> {
            v.into_iter()
                .map(|h| match h {
                    SearchHit::Favourite(i) | SearchHit::Window(i) => i,
                })
                .collect()
        };
        // App name matches every window of that app; the favourite wins ties.
        assert_eq!(ids(search_all("firefox", favs, wins)), ["f1", "w1", "w3"]);
        // Title words, and words split across title and app name.
        assert_eq!(ids(search_all("inbox", favs, wins)), ["w2"]);
        assert_eq!(ids(search_all("firefox rust", favs, wins)), ["w3"]);
        assert!(search_all("  ", favs, wins).is_empty());
        assert!(search_all("zzz", favs, wins).is_empty());
    }

    #[test]
    fn ranking() {
        assert!(score("fire", "Firefox").unwrap() > score("fox", "Firefox").unwrap());
        assert!(
            score("code", "Visual Studio Code").unwrap()
                > score("vsc", "Visual Studio Code").unwrap()
        );
        assert_eq!(score("zzz", "Firefox"), None);
        assert!(score("vis code", "Visual Studio Code").is_some());
        assert!(score("", "anything").is_some());
        assert_eq!(
            score_fields("browser", "Firefox", &["Web Browser"]),
            Some(40)
        );
    }
}
