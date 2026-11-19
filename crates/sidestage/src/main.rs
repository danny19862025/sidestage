//! Sidestage — favourite apps and minimized windows in one edge sidebar.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod brand;
mod glyphs;
mod settings;
mod sidebar;
mod single;
mod textures;
mod theme;
mod tray;

use std::sync::mpsc;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,usvg=error,eframe=warn,egui_glow=warn"),
    )
    .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("sidestage {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: sidestage [--settings] [--quit] [--autostart]\n\n  --settings   open Settings (in the running instance, if any)\n  --quit       quit the running instance\n  --autostart  started by the login autostart entry\n\nEnvironment:\n  SIDESTAGE_CONFIG    alternative config file path\n  SIDESTAGE_BACKEND   force a Linux backend: kwin | gnome | x11\n  RUST_LOG            log filter (e.g. debug)"
        );
        return Ok(());
    }
    let open_settings = args.iter().any(|a| a == "--settings");
    let quit = args.iter().any(|a| a == "--quit");

    let request = if quit {
        single::Request::Quit
    } else {
        single::Request::Settings
    };
    let mut guard = match single::acquire(request) {
        single::Instance::Primary(_) if quit => {
            log::info!("Sidestage is not running.");
            return Ok(());
        }
        single::Instance::Primary(g) => g,
        single::Instance::Secondary => {
            log::info!("Sidestage is already running; sent it the request.");
            return Ok(());
        }
    };

    // Capture the Wayland socket before the UI is pushed onto XWayland.
    #[cfg(target_os = "linux")]
    let _ = sidestage_platform::linux_wayland_display();
    sidestage_platform::init_worker_thread();
    for m in sidestage_platform::panel::monitors() {
        log::info!("monitor {m:?}");
    }

    let config = sidestage_core::Config::load(&sidestage_core::Config::default_path());
    let icon = brand::app_icon(64);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Sidestage")
        .with_app_id("sidestage")
        .with_decorations(false)
        .with_transparent(true)
        // Must stay resizable: winit pins min=max size hints otherwise, and
        // the WM would refuse our edge-to-edge height.
        .with_resizable(true)
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_always_on_top()
        .with_active(false)
        .with_window_type(egui::X11WindowType::Dock)
        .with_inner_size([config.settings.width + app::OUTER_MARGIN, 600.0])
        .with_position([0.0, 0.0])
        .with_icon(egui::IconData {
            rgba: icon.pixels,
            width: icon.width,
            height: icon.height,
        });

    let mut options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    {
        // The sidebar must be an X11 client to be placed at an edge and to
        // reserve space (see sidestage-platform/src/linux/panel.rs).
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    #[cfg(windows)]
    {
        let _ = &mut options;
    }

    eframe::run_native(
        "Sidestage",
        options,
        Box::new(move |cc| {
            let mut app = app::SidestageApp::new(cc, open_settings);
            let (tx, rx) = mpsc::channel();
            guard.listen(tx, cc.egui_ctx.clone());
            app.extra_commands(rx);
            app.apply_hotkey();
            app.textures
                .set_app_icon(&cc.egui_ctx, "__brand", &brand::app_icon(64));
            app.textures
                .set_app_icon(&cc.egui_ctx, "__brand_big", &brand::app_icon(256));
            Ok(Box::new(AppWithGuard { app, _guard: guard }))
        }),
    )
}

/// Keeps the single-instance guard alive as long as the app.
struct AppWithGuard {
    app: app::SidestageApp,
    _guard: single::Guard,
}

impl eframe::App for AppWithGuard {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame)
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame)
    }
    fn clear_color(&self, v: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(v)
    }
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.app.on_exit(gl)
    }
}
