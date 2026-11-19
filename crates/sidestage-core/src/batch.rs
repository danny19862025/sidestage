//! "Launch section": start several favourites in order, keep going when
//! one fails, and report what happened.

use crate::config::Favourite;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchReport {
    /// Ids of favourites that started, in launch order.
    pub launched: Vec<String>,
    /// `(name, error)` for favourites that failed, in launch order.
    pub failed: Vec<(String, String)>,
}

impl BatchReport {
    pub fn total(&self) -> usize {
        self.launched.len() + self.failed.len()
    }

    /// One line for a toast, e.g. "Launched 3 of 4 · Couldn't open Steam: …".
    pub fn summary(&self, section: &str) -> String {
        if self.failed.is_empty() {
            return format!(
                "Launched {} from {section}",
                plural(self.launched.len(), "app")
            );
        }
        let failures: Vec<String> = self
            .failed
            .iter()
            .map(|(n, e)| format!("{n} ({e})"))
            .collect();
        if self.launched.is_empty() {
            format!("Couldn't launch {section}: {}", failures.join("; "))
        } else {
            format!(
                "Launched {} of {} from {section} · Couldn't open: {}",
                self.launched.len(),
                self.total(),
                failures.join("; ")
            )
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Launches `items` in order with `launch`, continuing after failures.
pub fn run_batch<F>(items: &[Favourite], mut launch: F) -> BatchReport
where
    F: FnMut(&Favourite) -> Result<(), String>,
{
    let mut report = BatchReport::default();
    for fav in items {
        match launch(fav) {
            Ok(()) => report.launched.push(fav.id.clone()),
            Err(e) => report.failed.push((fav.name.clone(), e)),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IconSource, LaunchTarget};

    fn fav(name: &str) -> Favourite {
        Favourite::new(
            name,
            LaunchTarget::Command {
                program: name.into(),
                args: vec![],
            },
            IconSource::Auto,
        )
    }

    #[test]
    fn launches_in_display_order_and_skips_excluded() {
        let mut c = Config::default();
        let sec = c.sections[1].id.clone();
        for n in ["editor", "terminal", "browser", "notes"] {
            c.add_favourite(&sec, fav(n));
        }
        c.sections[1].items[2].include_in_launch = false; // browser
        let id = c.sections[1].items[3].id.clone();
        c.move_favourite(&id, &sec, 0); // notes first
        let list = c.section_launch_list(&sec);
        let mut order = Vec::new();
        let report = run_batch(&list, |f| {
            order.push(f.name.clone());
            Ok(())
        });
        assert_eq!(order, ["notes", "editor", "terminal"]);
        assert_eq!(report.launched.len(), 3);
        assert!(report.failed.is_empty());
        assert_eq!(report.summary("Coding"), "Launched 3 apps from Coding");
    }

    #[test]
    fn partial_failure_continues_and_reports() {
        let items: Vec<Favourite> = ["a", "broken", "c", "gone"]
            .iter()
            .map(|n| fav(n))
            .collect();
        let mut attempted = Vec::new();
        let report = run_batch(&items, |f| {
            attempted.push(f.name.clone());
            match f.name.as_str() {
                "broken" => Err("not installed".into()),
                "gone" => Err("file missing".into()),
                _ => Ok(()),
            }
        });
        assert_eq!(
            attempted,
            ["a", "broken", "c", "gone"],
            "every item is attempted"
        );
        assert_eq!(
            report.launched,
            vec![items[0].id.clone(), items[2].id.clone()]
        );
        assert_eq!(
            report.failed,
            vec![
                ("broken".into(), "not installed".into()),
                ("gone".into(), "file missing".into())
            ]
        );
        let s = report.summary("Work");
        assert!(s.starts_with("Launched 2 of 4 from Work"), "{s}");
        assert!(
            s.contains("broken (not installed)") && s.contains("gone (file missing)"),
            "{s}"
        );
    }

    #[test]
    fn all_failed_and_empty() {
        let items = vec![fav("x")];
        let r = run_batch(&items, |_| Err("nope".into()));
        assert_eq!(r.summary("Games"), "Couldn't launch Games: x (nope)");
        assert_eq!(run_batch(&[], |_| Ok(())).total(), 0);
    }
}
