//! Configs written by earlier releases must keep loading unchanged.

use sidestage_core::config::{normalize_web_url, url_display_name, Edge, MonitorChoice, Theme};
use sidestage_core::{Config, IconSource, LaunchTarget};

const V0_1: &str = include_str!("fixtures/config-v0.1.json");

fn load(text: &str) -> Config {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, text).unwrap();
    let c = Config::load(&path);
    // A file that loads must not be moved aside as "broken".
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    c
}

#[test]
fn v0_1_config_loads_with_every_field_preserved() {
    let c = load(V0_1);
    let s = &c.settings;
    assert_eq!(s.edge, Edge::Left);
    assert_eq!(
        s.monitor,
        MonitorChoice::Named {
            name: "eDP-1".into(),
            index: 0
        }
    );
    assert_eq!(
        (s.width, s.preview_height, s.icon_size),
        (250.0, 130.0, 44.0)
    );
    assert!(s.auto_hide && s.reserve_space && s.launch_at_login && s.show_labels && s.animations);
    assert_eq!(s.theme, Theme::System);
    assert_eq!(s.reveal_delay_ms, 375);
    // Settings added later get their defaults.
    assert!(s.pause_in_fullscreen);
    assert_eq!(s.search_shortcut, "Ctrl+Alt+Space");

    assert_eq!(c.sections.len(), 2);
    assert_eq!(c.sections[0].id, "sec-18a2b7c9d0e1f20");
    assert!(!c.sections[1].expanded);
    let game = &c.sections[0].items[1];
    assert_eq!(game.id, "fav-18a2b7c9d0e1f22");
    assert_eq!(
        game.target,
        LaunchTarget::Path {
            path: "/opt/games/game.AppImage".into(),
            args: vec!["--fullscreen".into()]
        }
    );
    assert_eq!(
        game.icon,
        IconSource::File {
            path: "/opt/games/icon.png".into()
        }
    );
    assert_eq!(
        c.sections[1].items[0].icon,
        IconSource::Themed {
            name: "utilities-terminal".into()
        }
    );
    // Missing "icon" (very old entries) defaults to Auto.
    assert_eq!(c.sections[1].items[1].icon, IconSource::Auto);
    assert_eq!(
        c.sections[1].items[1].target,
        LaunchTarget::AppsFolder {
            parsing_name: "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into()
        }
    );
}

#[test]
fn existing_favourites_are_included_in_section_launch() {
    let c = load(V0_1);
    assert!(c
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .all(|f| f.include_in_launch));
    let ids: Vec<_> = c
        .section_launch_list(&c.sections[0].id)
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(ids, ["Steam", "My Game"]);
}

#[test]
fn resave_round_trips_and_keeps_ids() {
    let c = load(V0_1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    c.save(&path).unwrap();
    let again = Config::load(&path);
    assert_eq!(again, c);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"include_in_launch\": true"));
    assert!(text.contains("fav-18a2b7c9d0e1f25"));
}

#[test]
fn folders_and_websites_round_trip() {
    let mut c = Config::default();
    let sec = c.sections[3].id.clone();
    let mut site = sidestage_core::Favourite::new(
        "Docs",
        LaunchTarget::Url {
            url: "https://docs.rs/".into(),
        },
        IconSource::Auto,
    );
    site.include_in_launch = false;
    c.add_favourite(&sec, site);
    c.add_favourite(
        &sec,
        sidestage_core::Favourite::new(
            "Projects",
            LaunchTarget::Path {
                path: "/home/u/Projects".into(),
                args: vec![],
            },
            IconSource::Auto,
        ),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    c.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"kind\": \"url\""));
    assert_eq!(Config::load(&path), c);
    assert_eq!(
        c.section_launch_list(&sec).len(),
        1,
        "excluded website is skipped by bulk launch"
    );
}

#[test]
fn web_url_validation() {
    assert_eq!(
        normalize_web_url(" example.com ").unwrap(),
        "https://example.com"
    );
    assert_eq!(
        normalize_web_url("http://localhost:8080/x").unwrap(),
        "http://localhost:8080/x"
    );
    assert_eq!(
        normalize_web_url("HTTPS://GitHub.com/a?b#c").unwrap(),
        "HTTPS://GitHub.com/a?b#c"
    );
    for bad in [
        "",
        "not a url",
        "ftp://x.org",
        "javascript:alert(1)",
        "file:///etc",
        "https://",
        "intranet",
    ] {
        assert!(normalize_web_url(bad).is_err(), "{bad} should be rejected");
    }
    assert_eq!(
        url_display_name("https://www.github.com/danny"),
        "github.com"
    );
}
