// Embeds the app icon and version information in sidestage.exe on Windows,
// so Explorer, the taskbar and "Installed apps" show the Sidestage logo.
// packaging/windows/sidestage.ico is rendered from src/brand.rs.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/windows/sidestage.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/windows/sidestage.ico");
        res.set("ProductName", "Sidestage");
        res.set("FileDescription", "Sidestage");
        res.set("CompanyName", "Sidestage contributors");
        res.set("LegalCopyright", "MIT License");
        res.compile().expect("cannot embed Windows resources (icon, version)");
    }
}
