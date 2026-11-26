//! The app's moment pictures (assets/art, made by tools/make_art.py): stills of the owner's own generated art, one per
//! kind of moment, embedded in the exe and turned into textures once. Nothing of AA Classic's art is in them.

use eframe::egui::{self, Color32, ColorImage, CornerRadius, Painter, Rect, TextureHandle, TextureOptions, pos2};

/// Every kind of moment; the shared moments of Community and Guild use the ones a duel does not.
#[allow(dead_code)]
pub struct Pictures {
    pub fog: TextureHandle,
    pub duel: TextureHandle,
    pub siege: TextureHandle,
    pub naval: TextureHandle,
    pub dawn: TextureHandle,
    pub port: TextureHandle,
    pub caravan: TextureHandle,
    pub harbor: TextureHandle,
    pub wall: TextureHandle,
    pub crest: TextureHandle,
}

fn load(ctx: &egui::Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let img = match image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg) {
        Ok(i) => i.to_rgb8(),
        // never in a build that passed its tests (the pictures are part of the exe): a dark square, not a crash
        Err(_) => image::RgbImage::from_pixel(4, 4, image::Rgb([20, 20, 24])),
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
            fog: pic!(ctx, "moment_fog"),
            duel: pic!(ctx, "moment_duel"),
            siege: pic!(ctx, "moment_siege"),
            naval: pic!(ctx, "moment_naval"),
            dawn: pic!(ctx, "moment_dawn"),
            port: pic!(ctx, "moment_port"),
            caravan: pic!(ctx, "moment_caravan"),
            harbor: pic!(ctx, "moment_harbor"),
            wall: pic!(ctx, "moment_wall"),
            crest: pic!(ctx, "crest"),
        }
    }

    /// The picture for one of the account's recordings. Today every recording is a fight between two players, so
    /// it is one of the two duel stills, the same one for the same recording every time.
    pub fn for_recording(&self, n: i32) -> &TextureHandle {
        if n.rem_euclid(2) == 0 { &self.duel } else { &self.dawn }
    }

    /// The welcome page's picture.
    pub fn welcome(&self) -> &TextureHandle {
        &self.fog
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

/// Draws `tex` filling `rect` (cropped, never stretched), tinted by `tint`, with rounded corners.
pub fn draw(p: &Painter, tex: &TextureHandle, rect: Rect, radius: impl Into<CornerRadius>, tint: Color32) {
    let shape = egui::epaint::RectShape::filled(rect, radius, tint).with_texture(tex.id(), cover(tex, rect));
    p.add(shape);
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_picture_in_the_exe_decodes() {
        for (name, bytes) in [
            ("moment_fog", &include_bytes!("../../assets/art/moment_fog.jpg")[..]),
            ("moment_duel", &include_bytes!("../../assets/art/moment_duel.jpg")[..]),
            ("moment_siege", &include_bytes!("../../assets/art/moment_siege.jpg")[..]),
            ("moment_naval", &include_bytes!("../../assets/art/moment_naval.jpg")[..]),
            ("moment_dawn", &include_bytes!("../../assets/art/moment_dawn.jpg")[..]),
            ("moment_port", &include_bytes!("../../assets/art/moment_port.jpg")[..]),
            ("moment_caravan", &include_bytes!("../../assets/art/moment_caravan.jpg")[..]),
            ("moment_harbor", &include_bytes!("../../assets/art/moment_harbor.jpg")[..]),
            ("moment_wall", &include_bytes!("../../assets/art/moment_wall.jpg")[..]),
            ("crest", &include_bytes!("../../assets/art/crest.jpg")[..]),
        ] {
            let i = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(i.width() >= 80 && i.height() >= 80, "{name}");
        }
    }
}
