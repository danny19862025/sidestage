//! Minimal freedesktop Desktop Entry parser (spec 1.5) with Exec expansion.
//! Kept platform-independent so it is unit-tested on every OS.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    pub path: PathBuf,
    /// Desktop file id, e.g. `org.kde.konsole` (file name without `.desktop`).
    pub id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub keywords: Vec<String>,
    pub icon: Option<String>,
    pub exec: Option<String>,
    pub try_exec: Option<String>,
    pub working_dir: Option<String>,
    pub terminal: bool,
    pub no_display: bool,
    pub hidden: bool,
    pub kind: String,
    /// `URL` of a `Type=Link` entry.
    pub url: Option<String>,
    pub startup_wm_class: Option<String>,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
}

impl DesktopEntry {
    pub fn parse(path: &Path, text: &str, locale: Option<&str>) -> Option<DesktopEntry> {
        let mut in_main = false;
        let mut raw: HashMap<String, String> = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_main = line == "[Desktop Entry]";
                continue;
            }
            if !in_main {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                raw.entry(k.trim().to_string())
                    .or_insert_with(|| v.trim().to_string());
            }
        }
        let get = |key: &str| -> Option<String> {
            if let Some(loc) = locale {
                for candidate in locale_variants(loc) {
                    if let Some(v) = raw.get(&format!("{key}[{candidate}]")) {
                        return Some(unescape(v));
                    }
                }
            }
            raw.get(key).map(|v| unescape(v))
        };
        let list = |key: &str| -> Vec<String> {
            get(key)
                .map(|v| {
                    v.split(';')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default()
        };
        let flag = |key: &str| {
            raw.get(key)
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
        };
        let hidden = flag("Hidden");
        // A user override that only says `Hidden=true` (spec: "deleted")
        // must still shadow the system entry, so Name is optional for it.
        let name = match get("Name") {
            Some(n) => n,
            None if hidden => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            None => return None,
        };
        Some(DesktopEntry {
            path: path.to_path_buf(),
            id: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            name,
            generic_name: get("GenericName"),
            comment: get("Comment"),
            keywords: list("Keywords"),
            icon: get("Icon").filter(|s| !s.is_empty()),
            exec: raw.get("Exec").cloned(),
            try_exec: raw.get("TryExec").cloned(),
            working_dir: raw.get("Path").cloned().filter(|s| !s.is_empty()),
            terminal: flag("Terminal"),
            no_display: flag("NoDisplay"),
            hidden,
            kind: raw.get("Type").cloned().unwrap_or_default(),
            url: raw
                .get("URL")
                .map(|v| unescape(v))
                .filter(|v| !v.is_empty()),
            startup_wm_class: raw.get("StartupWMClass").cloned(),
            only_show_in: list("OnlyShowIn"),
            not_show_in: list("NotShowIn"),
        })
    }

    pub fn load(path: &Path, locale: Option<&str>) -> Option<DesktopEntry> {
        let text = std::fs::read_to_string(path).ok()?;
        Self::parse(path, &text, locale)
    }

    /// Whether the entry belongs in an application list for `desktops`
    /// (the `XDG_CURRENT_DESKTOP` components).
    pub fn is_listable(&self, desktops: &[String]) -> bool {
        if self.kind != "Application" || self.no_display || self.hidden || self.exec.is_none() {
            return false;
        }
        let hit = |list: &[String]| {
            list.iter()
                .any(|d| desktops.iter().any(|c| c.eq_ignore_ascii_case(d)))
        };
        if !self.only_show_in.is_empty() && !hit(&self.only_show_in) {
            return false;
        }
        !hit(&self.not_show_in)
    }

    /// Expands `Exec` into argv with no file arguments.
    pub fn command_line(&self) -> Option<Vec<String>> {
        let exec = self.exec.as_deref()?;
        let argv = expand_exec(
            exec,
            self.icon.as_deref(),
            &self.name,
            &self.path.to_string_lossy(),
        )?;
        (!argv.is_empty()).then_some(argv)
    }
}

/// `de_DE.UTF-8@euro` -> [`de_DE@euro`, `de_DE`, `de@euro`, `de`].
fn locale_variants(locale: &str) -> Vec<String> {
    let (rest, modifier) = match locale.split_once('@') {
        Some((r, m)) => (r, Some(m)),
        None => (locale, None),
    };
    let rest = rest.split('.').next().unwrap_or(rest);
    let (lang, country) = match rest.split_once('_') {
        Some((l, c)) => (l, Some(c)),
        None => (rest, None),
    };
    let mut out = Vec::new();
    if let (Some(c), Some(m)) = (country, modifier) {
        out.push(format!("{lang}_{c}@{m}"));
    }
    if let Some(c) = country {
        out.push(format!("{lang}_{c}"));
    }
    if let Some(m) = modifier {
        out.push(format!("{lang}@{m}"));
    }
    out.push(lang.to_string());
    out
}

/// Desktop-entry string escapes: `\s \n \t \r \\`.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o)
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits an `Exec` value into argv following the spec's quoting rules and
/// expands field codes. File/URL codes are dropped because favourites launch
/// without arguments. Returns `None` for malformed quoting.
pub fn expand_exec(
    exec: &str,
    icon: Option<&str>,
    name: &str,
    location: &str,
) -> Option<Vec<String>> {
    // The value was not passed through `unescape` above; the general escape
    // rules apply first, then the quoting rules.
    let exec = unescape(exec);
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_cur = false;
    let mut chars = exec.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has_cur = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if has_cur {
                    args.push(std::mem::take(&mut cur));
                    has_cur = false;
                }
            }
            '%' => match chars.next() {
                Some('%') => {
                    cur.push('%');
                    has_cur = true;
                }
                Some('f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
                Some('i') => {
                    if let Some(icon) = icon {
                        if has_cur {
                            args.push(std::mem::take(&mut cur));
                            has_cur = false;
                        }
                        args.push("--icon".into());
                        args.push(icon.into());
                    }
                }
                Some('c') => {
                    cur.push_str(name);
                    has_cur = true;
                }
                Some('k') => {
                    cur.push_str(location);
                    has_cur = true;
                }
                Some(other) => {
                    cur.push('%');
                    cur.push(other);
                    has_cur = true;
                }
                None => {}
            },
            c => {
                cur.push(c);
                has_cur = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if has_cur {
        args.push(cur);
    }
    Some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KONSOLE: &str = "[Desktop Entry]\nType=Application\nName=Konsole\nName[de]=Konsole DE\nGenericName=Terminal\nIcon=utilities-terminal\nExec=konsole %U\nKeywords=shell;prompt;\nStartupWMClass=konsole\n\n[Desktop Action NewWindow]\nName=New Window\nExec=konsole --new\n";

    #[test]
    fn parses_main_group_only() {
        let e = DesktopEntry::parse(
            Path::new("/a/org.kde.konsole.desktop"),
            KONSOLE,
            Some("de_DE.UTF-8"),
        )
        .unwrap();
        assert_eq!(e.id, "org.kde.konsole");
        assert_eq!(e.name, "Konsole DE");
        assert_eq!(e.exec.as_deref(), Some("konsole %U"));
        assert_eq!(e.keywords, ["shell", "prompt"]);
        assert_eq!(e.command_line().unwrap(), ["konsole"]);
        assert!(e.is_listable(&["KDE".into()]));
    }

    #[test]
    fn exec_quoting_and_field_codes() {
        let argv = expand_exec(
            r#""/opt/My App/run" --name=%c %i "a \"q\"" 100%% %F"#,
            Some("ic"),
            "N",
            "/x",
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "/opt/My App/run",
                "--name=N",
                "--icon",
                "ic",
                "a \"q\"",
                "100%"
            ]
        );
        assert_eq!(expand_exec("\"broken", None, "", ""), None);
        assert_eq!(
            expand_exec("env  FOO=1   app", None, "", "").unwrap(),
            ["env", "FOO=1", "app"]
        );
        assert_eq!(
            expand_exec(r#"sh -c "echo \\$HOME""#, None, "", "").unwrap(),
            ["sh", "-c", "echo $HOME"]
        );
    }

    #[test]
    fn hidden_override_without_name_parses() {
        let e = DesktopEntry::parse(
            Path::new("/u/firefox.desktop"),
            "[Desktop Entry]\nHidden=true\n",
            None,
        )
        .unwrap();
        assert!(e.hidden);
        assert!(!e.is_listable(&[]));
        assert!(DesktopEntry::parse(
            Path::new("x.desktop"),
            "[Desktop Entry]\nType=Application\n",
            None
        )
        .is_none());
    }

    #[test]
    fn visibility_rules() {
        let mut e = DesktopEntry::parse(Path::new("x.desktop"), KONSOLE, None).unwrap();
        e.only_show_in = vec!["GNOME".into()];
        assert!(!e.is_listable(&["KDE".into()]));
        assert!(e.is_listable(&["ubuntu".into(), "GNOME".into()]));
        e.only_show_in.clear();
        e.no_display = true;
        assert!(!e.is_listable(&[]));
    }
}
