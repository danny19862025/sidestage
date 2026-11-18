//! GPU textures for favourite icons, window icons and previews, plus a
//! worker thread that loads favourite icons off the UI thread.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};

use egui::{ColorImage, TextureHandle, TextureOptions};
use sidestage_core::{Favourite, IconSource, LaunchTarget, RgbaImage, WindowId};

struct IconJob {
    key: String,
    icon: IconSource,
    target: LaunchTarget,
    size: u32,
}

pub struct Textures {
    icons: HashMap<String, TextureHandle>,
    thumbs: HashMap<WindowId, (TextureHandle, [usize; 2])>,
    requested: HashSet<String>,
    jobs: Sender<IconJob>,
    results: Receiver<(String, Option<RgbaImage>)>,
}

fn to_color_image(img: &RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels)
}

impl Textures {
    pub fn new(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<IconJob>();
        let (res_tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("icon-loader".into())
            .spawn(move || {
                sidestage_platform::init_worker_thread();
                while let Ok(job) = job_rx.recv() {
                    let img = sidestage_platform::icons::favourite_icon(&job.icon, &job.target, job.size);
                    if res_tx.send((job.key, img)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn icon loader");
        Self { icons: HashMap::new(), thumbs: HashMap::new(), requested: HashSet::new(), jobs, results }
    }

    /// Uploads finished icon loads. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, img)) = self.results.try_recv() {
            if let Some(img) = img {
                let tex = ctx.load_texture(&key, to_color_image(&img), TextureOptions::LINEAR);
                self.icons.insert(key, tex);
            }
        }
    }

    pub fn favourite_key(fav: &Favourite) -> String {
        // Include the icon source so changing the icon reloads it.
        format!("fav:{}:{:?}", fav.id, fav.icon)
    }

    /// Texture for a favourite, scheduling a load the first time.
    pub fn favourite(&mut self, fav: &Favourite, size_px: u32) -> Option<&TextureHandle> {
        let key = Self::favourite_key(fav);
        if !self.requested.contains(&key) {
            self.requested.insert(key.clone());
            let _ = self.jobs.send(IconJob { key: key.clone(), icon: fav.icon.clone(), target: fav.target.clone(), size: size_px });
        }
        self.icons.get(&key)
    }

    /// Preview of an icon source before it is saved (settings dialog).
    pub fn preview(&mut self, key: &str, icon: &IconSource, target: &LaunchTarget, size_px: u32) -> Option<&TextureHandle> {
        if !self.requested.contains(key) {
            self.requested.insert(key.to_string());
            let _ = self.jobs.send(IconJob { key: key.into(), icon: icon.clone(), target: target.clone(), size: size_px });
        }
        self.icons.get(key)
    }

    /// Forget all favourite icons, e.g. after the icon size changed.
    pub fn reload_favourites(&mut self) {
        self.icons.retain(|k, _| !k.starts_with("fav:") && !k.starts_with("app-entry:"));
        self.requested.retain(|k| !k.starts_with("fav:") && !k.starts_with("app-entry:"));
    }

    pub fn set_app_icon(&mut self, ctx: &egui::Context, app_key: &str, img: &RgbaImage) {
        let key = format!("app:{app_key}");
        let tex = ctx.load_texture(&key, to_color_image(img), TextureOptions::LINEAR);
        self.icons.insert(key, tex);
    }

    pub fn app_icon(&self, app_key: &str) -> Option<&TextureHandle> {
        self.icons.get(&format!("app:{app_key}"))
    }

    pub fn set_thumbnail(&mut self, ctx: &egui::Context, id: &WindowId, img: Option<&RgbaImage>) {
        match img {
            Some(img) => {
                let tex = ctx.load_texture(format!("thumb:{id}"), to_color_image(img), TextureOptions::LINEAR);
                self.thumbs.insert(id.clone(), (tex, [img.width as usize, img.height as usize]));
            }
            None => {
                self.thumbs.remove(id);
            }
        }
    }

    pub fn thumbnail(&self, id: &WindowId) -> Option<&(TextureHandle, [usize; 2])> {
        self.thumbs.get(id)
    }

    pub fn retain_thumbnails(&mut self, keep: impl Fn(&WindowId) -> bool) {
        self.thumbs.retain(|id, _| keep(id));
    }
}
