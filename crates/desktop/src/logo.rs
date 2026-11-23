//! The xsim logo (D20): the owner's blue wave behind "xsim", shown at the
//! left of the title bar.
//!
//! `assets/make_logo.py` renders it from `assets/logo-source.webp` on a
//! transparent background, in two variants: white letters for the dark
//! theme and dark letters for the light theme, each at a few heights. GPUI
//! shrinks images on the GPU without mipmaps, which turns a large image
//! drawn a few pixels high grainy, so the title bar uses the smallest
//! rendering that covers its height in device pixels. At 100 % on a 100 %
//! display that is the 24 px one, drawn texel for texel.

use gpui_kit::{Image, ImageFormat, Pixels};
use std::sync::{Arc, LazyLock};

/// One rendering of the logo.
pub struct Logo {
    /// Size in pixels, from the PNG header.
    pub width: u32,
    pub height: u32,
    pub image: Arc<Image>,
}

impl Logo {
    fn new(png: &'static [u8]) -> Self {
        let (width, height) = png_size(png).unwrap_or((0, 0));
        Self {
            width,
            height,
            image: Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())),
        }
    }

    /// The width that keeps the logo's proportions at `height`.
    pub fn width_at(&self, height: Pixels) -> Pixels {
        if self.height == 0 {
            return height;
        }
        height * (self.width as f32 / self.height as f32)
    }
}

/// Number of renderings per variant.
const RENDERINGS: usize = 6;

/// White letters, for dark title bars; smallest first.
const DARK_THEME_PNGS: [&[u8]; RENDERINGS] = [
    include_bytes!("../assets/logo-dark-24.png"),
    include_bytes!("../assets/logo-dark-32.png"),
    include_bytes!("../assets/logo-dark-48.png"),
    include_bytes!("../assets/logo-dark-64.png"),
    include_bytes!("../assets/logo-dark-96.png"),
    include_bytes!("../assets/logo-dark-128.png"),
];

/// Dark letters, for light title bars; smallest first.
const LIGHT_THEME_PNGS: [&[u8]; RENDERINGS] = [
    include_bytes!("../assets/logo-light-24.png"),
    include_bytes!("../assets/logo-light-32.png"),
    include_bytes!("../assets/logo-light-48.png"),
    include_bytes!("../assets/logo-light-64.png"),
    include_bytes!("../assets/logo-light-96.png"),
    include_bytes!("../assets/logo-light-128.png"),
];

/// Built once, so every frame hands GPUI the same images and its cache
/// decodes each only once.
static DARK_THEME: LazyLock<[Logo; RENDERINGS]> = LazyLock::new(|| DARK_THEME_PNGS.map(Logo::new));
static LIGHT_THEME: LazyLock<[Logo; RENDERINGS]> =
    LazyLock::new(|| LIGHT_THEME_PNGS.map(Logo::new));

/// The rendering for a logo `device_height` device pixels high on a dark
/// or light title bar: the smallest at least that high, or the largest.
pub fn logo_for(dark: bool, device_height: f32) -> &'static Logo {
    let renderings: &'static [Logo; RENDERINGS] = if dark { &DARK_THEME } else { &LIGHT_THEME };
    renderings
        .iter()
        .find(|logo| logo.height as f32 >= device_height)
        .unwrap_or(&renderings[RENDERINGS - 1])
}

/// Width and height from a PNG's header (its first chunk, IHDR).
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if png.get(..8)? != SIGNATURE || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(png.get(at..at + 4)?.try_into().ok()?))
    };
    Some((word(16)?, word(20)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext, px};

    /// Bit depth and colour type from a PNG's header.
    fn png_format(png: &[u8]) -> (u8, u8) {
        (png[24], png[25])
    }

    #[test]
    fn every_rendering_is_an_rgba_png_of_the_same_shape() {
        for (pngs, theme) in [(DARK_THEME_PNGS, "dark"), (LIGHT_THEME_PNGS, "light")] {
            let mut previous = 0;
            for png in pngs {
                let (width, height) = png_size(png).expect("a PNG header");
                assert!(height > previous, "{theme}: ordered by height");
                previous = height;
                // 8-bit RGBA: the background is transparent.
                assert_eq!(png_format(png), (8, 6), "{theme} {height}");
                let aspect = width as f32 / height as f32;
                assert!(
                    (aspect - 1.915).abs() < 0.02,
                    "{theme} {height}: {width}x{height}"
                );
            }
        }
        assert_eq!(
            DARK_THEME_PNGS.map(png_size),
            LIGHT_THEME_PNGS.map(png_size),
            "both variants come in the same sizes"
        );
    }

    #[test]
    fn the_smallest_rendering_that_covers_the_height_is_used() {
        assert_eq!(logo_for(true, 18.0).height, 24, "75 %");
        assert_eq!(logo_for(true, 24.0).height, 24, "100 %: texel for texel");
        assert_eq!(logo_for(true, 25.0).height, 32);
        assert_eq!(logo_for(true, 32.0).height, 32);
        assert_eq!(logo_for(true, 33.0).height, 48);
        assert_eq!(logo_for(false, 52.0).height, 64);
        assert_eq!(logo_for(false, 97.0).height, 128);
        assert_eq!(logo_for(false, 500.0).height, 128, "the largest beyond");
        // GPUI never shrinks one by more than 1.5x (the title bar's logo
        // is at least 18 px high, at 75 %).
        for dark in [false, true] {
            for device in 16..=128 {
                let logo = logo_for(dark, device as f32);
                assert!(logo.height as f32 <= (device as f32 * 1.5).max(24.0));
            }
        }
    }

    #[test]
    fn the_variants_differ_and_keep_their_proportions() {
        let (dark, light) = (logo_for(true, 24.0), logo_for(false, 24.0));
        assert_ne!(dark.image.id(), light.image.id());
        assert_eq!(
            logo_for(true, 24.0).image.id(),
            dark.image.id(),
            "built once"
        );
        // At its own height a rendering is as wide as its PNG.
        assert_eq!((dark.width, dark.height), (46, 24));
        assert_eq!(dark.width_at(px(24.)), px(46.));
        let larger = logo_for(true, 30.0);
        assert_eq!((larger.width, larger.height), (61, 32));
        let width = larger.width_at(px(30.));
        assert!((width - px(30. * 61. / 32.)).abs() < px(0.01), "{width:?}");
    }

    /// GPUI draws an image with straight alpha and bilinear sampling, so
    /// where it shows a rendering a little smaller than its PNG, the
    /// texels beside the drawing blend into its edge whatever their alpha.
    /// They carry the colour of the drawing next to them, not black, so
    /// the white letters and the wave get no dark rim (D20).
    #[gpui_kit::test]
    fn the_texels_around_the_drawing_carry_its_colour(cx: &mut TestAppContext) {
        // Texels at least this opaque are the drawing; `make_logo.py`
        // gives its colour to the texels around it.
        const DRAWN: u8 = 16;
        let cx = cx.add_empty_window();
        for (renderings, theme) in [(&*DARK_THEME, "dark"), (&*LIGHT_THEME, "light")] {
            for logo in renderings {
                let bgra = decoded(logo, cx);
                let (width, height) = (logo.width as usize, logo.height as usize);
                assert_eq!(bgra.len(), width * height * 4, "{theme} {height}");
                let texel = |x: usize, y: usize| -> &[u8] { &bgra[(y * width + x) * 4..][..4] };
                let mut beside = 0;
                for y in 0..height {
                    for x in 0..width {
                        let own = texel(x, y);
                        if own[3] >= DRAWN {
                            continue;
                        }
                        let drawn: Vec<&[u8]> = (y.saturating_sub(1)..(y + 2).min(height))
                            .flat_map(|ny| {
                                (x.saturating_sub(1)..(x + 2).min(width)).map(move |nx| (nx, ny))
                            })
                            .map(|(nx, ny)| texel(nx, ny))
                            .filter(|neighbour| neighbour[3] >= DRAWN)
                            .collect();
                        if drawn.is_empty() {
                            continue;
                        }
                        beside += 1;
                        // Within the colours of the drawing beside it (one
                        // step of rounding either way), channel by channel.
                        for channel in 0..3 {
                            let low = drawn.iter().map(|n| n[channel]).min().unwrap_or(0);
                            let high = drawn.iter().map(|n| n[channel]).max().unwrap_or(255);
                            assert!(
                                own[channel] >= low.saturating_sub(1)
                                    && own[channel] <= high.saturating_add(1),
                                "{theme} {height}: texel ({x}, {y}) {own:?} is not coloured \
                                 like the drawing beside it {drawn:?}"
                            );
                        }
                    }
                }
                assert!(beside > width, "{theme} {height}: the drawing has an edge");
            }
        }
    }

    /// The BGRA pixels GPUI decodes from `logo`'s PNG.
    fn decoded(logo: &Logo, cx: &mut VisualTestContext) -> Vec<u8> {
        for _ in 0..10 {
            let image = cx.update(|window, cx| logo.image.clone().get_render_image(window, cx));
            if let Some(image) = image {
                return image.as_bytes(0).expect("one frame").to_vec();
            }
            cx.run_until_parked();
        }
        panic!("GPUI does not decode the {}-pixel logo", logo.height);
    }

    #[test]
    fn a_broken_header_has_no_size() {
        assert_eq!(png_size(b"not a png"), None);
        assert_eq!(png_size(&DARK_THEME_PNGS[0][..20]), None);
    }

    /// The Windows icon (`assets/xsim.ico`, embedded by `build.rs`) holds
    /// every size Windows asks for.
    #[test]
    fn the_windows_icon_has_every_size() {
        let ico: &[u8] = include_bytes!("../assets/xsim.ico");
        let word = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
        assert_eq!((word(0), word(2)), (0, 1), "an icon file");
        let count = usize::from(word(4));
        let sizes: Vec<u32> = (0..count)
            .map(|i| match ico[6 + 16 * i] {
                0 => 256,
                size => u32::from(size),
            })
            .collect();
        assert_eq!(sizes, [16, 24, 32, 48, 64, 128, 256]);
        for i in 0..count {
            let entry = 6 + 16 * i;
            let bits = word(entry + 6);
            let length =
                u32::from_le_bytes(ico[entry + 8..entry + 12].try_into().expect("4 bytes"));
            let offset =
                u32::from_le_bytes(ico[entry + 12..entry + 16].try_into().expect("4 bytes"));
            assert_eq!(bits, 32, "frame {i} has alpha");
            assert!(
                (offset + length) as usize <= ico.len(),
                "frame {i} is whole"
            );
        }
    }
}
