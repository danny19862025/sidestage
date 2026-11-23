//! "Check for updates" on the About page: asks GitHub for the latest
//! release, on a background thread, only when the user clicks the button.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// The release page, for "Download".
pub const RELEASES_URL: &str = "https://github.com/danny19862025/sidestage/releases/latest";
const API_URL: &str = "https://api.github.com/repos/danny19862025/sidestage/releases/latest";

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate {
        latest: String,
    },
    Available {
        latest: String,
        url: String,
    },
    Failed(String),
}

#[derive(Default)]
pub struct UpdateCheck {
    pub state: UpdateState,
    rx: Option<Receiver<UpdateState>>,
}

impl UpdateCheck {
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.state == UpdateState::Checking {
            return;
        }
        self.state = UpdateState::Checking;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check());
            ctx.request_repaint();
        });
    }

    /// Picks up a finished check.
    pub fn poll(&mut self) {
        if let Some(state) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.state = state;
            self.rx = None;
        }
    }
}

fn check() -> UpdateState {
    match latest_release() {
        Ok((tag, url)) => {
            let latest = tag.trim_start_matches(['v', 'V']).to_string();
            if is_newer(&latest, env!("CARGO_PKG_VERSION")) {
                UpdateState::Available { latest, url }
            } else {
                UpdateState::UpToDate { latest }
            }
        }
        Err(e) => {
            log::warn!("update check failed: {e}");
            UpdateState::Failed(e)
        }
    }
}

/// (tag, release page URL) of the latest GitHub release.
fn latest_release() -> Result<(String, String), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let mut resp = agent
        .get(API_URL)
        .header("User-Agent", concat!("Sidestage/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => "no releases have been published yet".to_string(),
            ureq::Error::StatusCode(403) | ureq::Error::StatusCode(429) => {
                "GitHub is limiting requests; try again later".to_string()
            }
            e => format!("couldn't reach GitHub ({e})"),
        })?;
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("couldn't read GitHub's reply ({e})"))?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("unexpected reply from GitHub ({e})"))?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or("the latest release has no version tag")?
        .to_string();
    let url = json["html_url"].as_str().unwrap_or(RELEASES_URL).to_string();
    Ok((tag, url))
}

/// Numeric `major.minor.patch` comparison; anything after `-` or `+` is
/// ignored, missing parts count as 0.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
    }
}
