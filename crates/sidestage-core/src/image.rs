/// A straight-alpha RGBA8 bitmap handed from platform code to the UI.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for RgbaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RgbaImage({}x{})", self.width, self.height)
    }
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (width > 0 && height > 0 && pixels.len() == (width * height * 4) as usize)
            .then_some(Self { width, height, pixels })
    }

    /// Converts premultiplied or straight BGRA (Win32/X11/Cairo order) to RGBA.
    pub fn from_bgra(width: u32, height: u32, mut pixels: Vec<u8>, premultiplied: bool) -> Option<Self> {
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
            if premultiplied && px[3] != 0 && px[3] != 255 {
                let a = px[3] as u32;
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Self::new(width, height, pixels)
    }

    /// True when every pixel is fully transparent (e.g. a failed capture).
    pub fn is_blank(&self) -> bool {
        self.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0)
    }

    /// Nearest-neighbour downscale so the longest side is at most `max`.
    /// Thumbnails are shown small, so this keeps texture uploads cheap.
    pub fn downscaled(&self, max: u32) -> RgbaImage {
        let longest = self.width.max(self.height);
        if longest <= max {
            return self.clone();
        }
        let w = (self.width as u64 * max as u64 / longest as u64).max(1) as u32;
        let h = (self.height as u64 * max as u64 / longest as u64).max(1) as u32;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            // Box-filter 2x2 samples around the nearest pixel for less shimmer.
            let sy = (y as u64 * self.height as u64 / h as u64) as u32;
            let sy2 = ((sy + 1).min(self.height - 1)) as usize;
            for x in 0..w {
                let sx = (x as u64 * self.width as u64 / w as u64) as u32;
                let sx2 = ((sx + 1).min(self.width - 1)) as usize;
                let idx = |xx: usize, yy: usize| (yy * self.width as usize + xx) * 4;
                let s = [
                    idx(sx as usize, sy as usize),
                    idx(sx2, sy as usize),
                    idx(sx as usize, sy2),
                    idx(sx2, sy2),
                ];
                for c in 0..4 {
                    let sum: u32 = s.iter().map(|&i| self.pixels[i + c] as u32).sum();
                    out.push((sum / 4) as u8);
                }
            }
        }
        RgbaImage { width: w, height: h, pixels: out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_conversion_and_unpremultiply() {
        let img = RgbaImage::from_bgra(1, 1, vec![10, 20, 100, 128], true).unwrap();
        assert_eq!(img.pixels[3], 128);
        assert_eq!(img.pixels[0], 199); // 100 * 255 / 128 rounded
        assert_eq!(img.pixels[2], 20);
    }

    #[test]
    fn downscale_keeps_aspect() {
        let img = RgbaImage::new(400, 200, vec![255; 400 * 200 * 4]).unwrap();
        let small = img.downscaled(100);
        assert_eq!((small.width, small.height), (100, 50));
        assert!(!small.is_blank());
    }
}
