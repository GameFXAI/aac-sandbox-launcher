//! The launcher's pictures: crops of the owner's AA REPLAY paintings (assets/art, made by tools/make_art.py), embedded
//! in the exe and turned into textures once. Nothing of AA Classic's art is in them.

use eframe::egui::{self, Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions, pos2};

pub struct Pictures {
    pub hero_play: TextureHandle,
    pub hero_replays: TextureHandle,
    pub hero_more: TextureHandle,
    pub won: TextureHandle,
    pub lost: TextureHandle,
    pub fight: TextureHandle,
    pub won_grey: TextureHandle,
    pub lost_grey: TextureHandle,
    pub fight_grey: TextureHandle,
}

fn load(ctx: &egui::Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let img = match image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg) {
        Ok(i) => i.to_rgb8(),
        // never in a build that passed its tests (the pictures are part of the exe): a dark square, not a crash
        Err(_) => image::RgbImage::from_pixel(4, 4, image::Rgb([20, 15, 11])),
    };
    let ci = ColorImage::from_rgb([img.width() as usize, img.height() as usize], img.as_raw());
    ctx.load_texture(name, ci, TextureOptions::LINEAR)
}

macro_rules! pic {
    ($ctx:expr, $name:literal) => {
        load($ctx, $name, include_bytes!(concat!("../../assets/art/", $name, ".jpg")))
    };
}

impl Pictures {
    pub fn load(ctx: &egui::Context) -> Pictures {
        Pictures {
            hero_play: pic!(ctx, "hero_play"),
            hero_replays: pic!(ctx, "hero_replays"),
            hero_more: pic!(ctx, "hero_more"),
            won: pic!(ctx, "thumb_won"),
            lost: pic!(ctx, "thumb_lost"),
            fight: pic!(ctx, "thumb_fight"),
            won_grey: pic!(ctx, "thumb_won_grey"),
            lost_grey: pic!(ctx, "thumb_lost_grey"),
            fight_grey: pic!(ctx, "thumb_fight_grey"),
        }
    }
}

/// The part of a picture that fills `rect` without stretching it (the middle of it, like CSS `cover`).
pub fn cover(tex: &TextureHandle, rect: Rect) -> Rect {
    let [w, h] = tex.size();
    let (img, want) = (w as f32 / h as f32, rect.width() / rect.height());
    if want > img {
        let v = img / want;
        Rect::from_min_max(pos2(0.0, (1.0 - v) / 2.0), pos2(1.0, (1.0 + v) / 2.0))
    } else {
        let u = want / img;
        Rect::from_min_max(pos2((1.0 - u) / 2.0, 0.0), pos2((1.0 + u) / 2.0, 1.0))
    }
}

/// Draws `tex` filling `rect` (cropped, never stretched), darkened by `tint`.
pub fn draw(p: &Painter, tex: &TextureHandle, rect: Rect, tint: Color32) {
    p.image(tex.id(), rect, cover(tex, rect), tint);
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_picture_in_the_exe_decodes() {
        for (name, bytes) in [
            ("hero_play", &include_bytes!("../../assets/art/hero_play.jpg")[..]),
            ("hero_replays", &include_bytes!("../../assets/art/hero_replays.jpg")[..]),
            ("hero_more", &include_bytes!("../../assets/art/hero_more.jpg")[..]),
            ("thumb_won", &include_bytes!("../../assets/art/thumb_won.jpg")[..]),
            ("thumb_lost_grey", &include_bytes!("../../assets/art/thumb_lost_grey.jpg")[..]),
        ] {
            let i = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(i.width() >= 384 && i.height() >= 240, "{name}");
        }
    }
}
