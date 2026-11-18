//! Installed applications from the shell's AppsFolder (Start menu apps,
//! including Store/UWP apps), and dropped-file conversion.

use std::path::Path;

use sidestage_core::{IconSource, LaunchTarget};
use windows::core::HSTRING;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, IShellItem2, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING,
};

use crate::apps::AppEntry;

/// PKEY_AppUserModel_ID {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5
pub(crate) const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

fn display_name(item: &IShellItem, kind: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

pub fn installed_apps() -> Vec<AppEntry> {
    let mut out = Vec::new();
    unsafe {
        let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from("shell:AppsFolder"), None) else {
            return out;
        };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut fetched = 0u32;
            let mut slot: [Option<IShellItem>; 1] = [None];
            if items.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else { break };
            let Some(name) = display_name(&item, SIGDN_NORMALDISPLAY) else { continue };
            // The AUMID launches the app through shell:AppsFolder\<id>.
            let aumid = windows::core::Interface::cast::<IShellItem2>(&item)
                .ok()
                .and_then(|i2| i2.GetString(&PKEY_APP_USER_MODEL_ID).ok())
                .and_then(|p| {
                    let s = p.to_string().ok();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
                .or_else(|| display_name(&item, SIGDN_PARENTRELATIVEPARSING));
            let Some(aumid) = aumid.filter(|a| !a.is_empty()) else { continue };
            if name.to_lowercase().starts_with("uninstall") {
                continue;
            }
            // Desktop apps report their exe path; show the file name as a hint.
            let subtitle = if aumid.contains('\\') {
                Path::new(&aumid).file_name().map(|f| f.to_string_lossy().into_owned())
            } else {
                Some("Microsoft Store app".to_string())
            };
            out.push(AppEntry {
                keywords: vec![aumid.clone()],
                name,
                subtitle,
                target: LaunchTarget::AppsFolder { parsing_name: aumid },
                icon: IconSource::Auto,
            });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.target == b.target);
    out
}

pub fn from_path(path: &Path) -> Option<AppEntry> {
    if !path.exists() {
        return None;
    }
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let subtitle = Some(path.display().to_string());
    Some(AppEntry {
        name,
        subtitle,
        target: LaunchTarget::Path { path: path.to_path_buf(), args: vec![] },
        icon: IconSource::Auto,
        keywords: vec![],
    })
}
