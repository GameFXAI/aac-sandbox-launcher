//! The app's line icons, painted in code (1.5 px strokes in a 20-unit box): no picture files, nothing of AA Classic's.

use super::theme::{GOLD, MUTED, RED, rgba};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// The whole set the designs use; the Community and Guild pages draw the ones My captures does not.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Folder,
    People,
    Shield,
    Compass,
    Rss,
    Star,
    Bookmark,
    BookmarkFill,
    Share,
    Link,
    External,
    Search,
    Filter,
    Grid,
    List,
    Sort,
    ChevronDown,
    ChevronRight,
    Pencil,
    Lock,
    Check,
    CheckCircle,
    Home,
    Layers,
    CalendarCheck,
    UserPlus,
    Chat,
    Warning,
    Gamepad,
    Close,
    Minimize,
    Maximize,
    Restore,
    Monitor,
    Play,
    Clock,
    Refresh,
    Film,
    Trash,
    Gear,
    SignOut,
    Flag,
    Download,
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        c + vec2(r * a.cos(), r * a.sin())
    }).collect()
}

/// Paints `kind` centred on `c`, `s` pixels across, in `color`.
pub fn icon(p: &Painter, c: Pos2, s: f32, kind: Icon, color: Color32) {
    let k = s / 20.0;
    let line = Stroke::new((1.5 * k).max(1.15), color);
    let at = |x: f32, y: f32| c + vec2(x * k, y * k);
    let poly = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), line);
    let closed = |pts: &[(f32, f32)]| Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), line);
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| Shape::rect_stroke(Rect::from_min_max(at(x0, y0), at(x1, y1)), r * k, line, egui::StrokeKind::Middle);
    match kind {
        Icon::Folder => {
            p.add(closed(&[(-8.5, -6.0), (-3.5, -6.0), (-1.5, -4.0), (8.5, -4.0), (8.5, 7.0), (-8.5, 7.0)]));
            p.add(poly(&[(-8.5, -1.5), (8.5, -1.5)]));
        }
        Icon::People => {
            p.circle_stroke(at(-3.0, -3.5), 3.2 * k, line);
            p.add(Shape::line(arc(at(-3.0, 8.5), 6.5 * k, PI + 0.25, TAU - 0.25, 14), line));
            p.add(Shape::line(arc(at(4.5, -3.0), 2.8 * k, -2.3, 1.4, 10), line));
            p.add(poly(&[(5.5, 2.5), (9.0, 4.5), (9.5, 7.5)]));
        }
        Icon::Shield => {
            p.add(closed(&[(0.0, -9.0), (7.5, -6.0), (7.0, 1.5), (0.0, 9.0), (-7.0, 1.5), (-7.5, -6.0)]));
        }
        Icon::Compass => {
            p.circle_stroke(c, 8.5 * k, line);
            p.add(closed(&[(3.5, -3.5), (1.2, 1.2), (-3.5, 3.5), (-1.2, -1.2)]));
        }
        Icon::Rss => {
            p.circle_filled(at(-6.0, 6.0), 1.4 * k, color);
            p.add(Shape::line(arc(at(-7.5, 7.5), 7.0 * k, -FRAC_PI_2, 0.0, 10), line));
            p.add(Shape::line(arc(at(-7.5, 7.5), 13.0 * k, -FRAC_PI_2, 0.0, 14), line));
        }
        Icon::Star => {
            let pts: Vec<(f32, f32)> = (0..10).map(|i| {
                let a = -FRAC_PI_2 + i as f32 * PI / 5.0;
                let r = if i % 2 == 0 { 9.0 } else { 4.0 };
                (r * a.cos(), r * a.sin())
            }).collect();
            p.add(closed(&pts));
        }
        Icon::Bookmark => {
            p.add(closed(&[(-5.5, -8.5), (5.5, -8.5), (5.5, 8.5), (0.0, 4.0), (-5.5, 8.5)]));
        }
        Icon::BookmarkFill => {
            p.add(Shape::convex_polygon(vec![at(-5.5, -8.5), at(5.5, -8.5), at(5.5, 8.5), at(0.0, 4.0)], color, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![at(-5.5, -8.5), at(0.0, 4.0), at(-5.5, 8.5)], color, Stroke::NONE));
        }
        Icon::Share => {
            p.add(poly(&[(0.0, 3.0), (0.0, -9.0)]));
            p.add(poly(&[(-4.0, -5.0), (0.0, -9.0), (4.0, -5.0)]));
            p.add(poly(&[(-4.5, -1.5), (-7.5, -1.5), (-7.5, 8.5), (7.5, 8.5), (7.5, -1.5), (4.5, -1.5)]));
        }
        Icon::Link => {
            for (dx, dy) in [(-3.0, 3.0), (3.0, -3.0)] {
                let ctr = at(dx, dy);
                let d = vec2(0.7071, -0.7071);
                let n = vec2(0.7071, 0.7071);
                let (hl, hw) = (5.0 * k, 3.0 * k);
                p.add(Shape::closed_line(vec![ctr - d * hl - n * hw, ctr + d * hl - n * hw, ctr + d * hl + n * hw, ctr - d * hl + n * hw], line));
            }
        }
        Icon::External => {
            p.add(poly(&[(-6.0, 6.0), (6.5, -6.5)]));
            p.add(poly(&[(-2.0, -6.5), (6.5, -6.5), (6.5, 2.0)]));
        }
        Icon::Search => {
            p.circle_stroke(at(-1.5, -1.5), 6.0 * k, line);
            p.add(poly(&[(3.0, 3.0), (8.0, 8.0)]));
        }
        Icon::Filter => {
            p.add(closed(&[(-8.5, -7.0), (8.5, -7.0), (2.0, 1.0), (2.0, 7.5), (-2.0, 5.5), (-2.0, 1.0)]));
        }
        Icon::Grid => {
            for (x, y) in [(-8.0, -8.0), (1.5, -8.0), (-8.0, 1.5), (1.5, 1.5)] {
                p.add(rrect(x, y, x + 6.5, y + 6.5, 1.5));
            }
        }
        Icon::List => {
            for y in [-6.0, 0.0, 6.0] {
                p.circle_filled(at(-7.5, y), 1.1 * k, color);
                p.add(poly(&[(-4.0, y), (8.5, y)]));
            }
        }
        Icon::Sort => {
            p.add(poly(&[(-4.0, 8.0), (-4.0, -8.0)]));
            p.add(poly(&[(-7.5, -4.5), (-4.0, -8.0), (-0.5, -4.5)]));
            p.add(poly(&[(4.0, -8.0), (4.0, 8.0)]));
            p.add(poly(&[(0.5, 4.5), (4.0, 8.0), (7.5, 4.5)]));
        }
        Icon::ChevronDown => {
            p.add(poly(&[(-5.0, -2.5), (0.0, 2.5), (5.0, -2.5)]));
        }
        Icon::ChevronRight => {
            p.add(poly(&[(-2.5, -5.0), (2.5, 0.0), (-2.5, 5.0)]));
        }
        Icon::Pencil => {
            p.add(closed(&[(-7.5, 7.5), (-7.0, 3.5), (4.0, -7.5), (7.5, -4.0), (-3.5, 7.0)]));
            p.add(poly(&[(1.5, -5.0), (5.0, -1.5)]));
        }
        Icon::Lock => {
            p.add(rrect(-6.5, -1.5, 6.5, 8.5, 2.0));
            p.add(Shape::line(arc(at(0.0, -2.0), 4.2 * k, PI, TAU, 12), line));
            p.add(poly(&[(-4.2, -2.0), (-4.2, -1.5)]));
            p.add(poly(&[(4.2, -2.0), (4.2, -1.5)]));
            p.add(poly(&[(0.0, 2.5), (0.0, 4.5)]));
        }
        Icon::Check => {
            p.add(poly(&[(-7.0, 0.5), (-2.5, 5.0), (7.5, -5.5)]));
        }
        Icon::CheckCircle => {
            p.circle_stroke(c, 8.5 * k, line);
            p.add(poly(&[(-4.0, 0.5), (-1.0, 3.5), (4.5, -3.0)]));
        }
        Icon::Home => {
            p.add(closed(&[(-8.0, -1.0), (0.0, -8.5), (8.0, -1.0), (6.0, -1.0), (6.0, 8.0), (-6.0, 8.0), (-6.0, -1.0)]));
            p.add(poly(&[(-2.0, 8.0), (-2.0, 3.0), (2.0, 3.0), (2.0, 8.0)]));
        }
        Icon::Layers => {
            p.add(closed(&[(0.0, -8.5), (8.5, -4.0), (0.0, 0.5), (-8.5, -4.0)]));
            p.add(poly(&[(-8.5, 0.0), (0.0, 4.5), (8.5, 0.0)]));
            p.add(poly(&[(-8.5, 4.0), (0.0, 8.5), (8.5, 4.0)]));
        }
        Icon::CalendarCheck => {
            p.add(rrect(-7.5, -6.0, 7.5, 8.5, 2.0));
            p.add(poly(&[(-7.5, -2.0), (7.5, -2.0)]));
            p.add(poly(&[(-3.5, -8.5), (-3.5, -4.5)]));
            p.add(poly(&[(3.5, -8.5), (3.5, -4.5)]));
            p.add(poly(&[(-3.5, 3.0), (-1.0, 5.5), (3.5, 1.0)]));
        }
        Icon::UserPlus => {
            p.circle_stroke(at(-3.0, -3.5), 3.5 * k, line);
            p.add(Shape::line(arc(at(-3.0, 9.0), 7.0 * k, PI + 0.3, TAU - 0.3, 14), line));
            p.add(poly(&[(6.0, -4.0), (6.0, 2.0)]));
            p.add(poly(&[(3.0, -1.0), (9.0, -1.0)]));
        }
        Icon::Chat => {
            p.add(closed(&[(-8.0, -7.0), (8.0, -7.0), (8.0, 4.0), (-2.0, 4.0), (-6.0, 8.0), (-6.0, 4.0), (-8.0, 4.0)]));
        }
        Icon::Warning => {
            p.add(closed(&[(0.0, -8.5), (9.0, 7.5), (-9.0, 7.5)]));
            p.add(poly(&[(0.0, -2.5), (0.0, 2.5)]));
            p.circle_filled(at(0.0, 5.0), 1.0 * k, color);
        }
        Icon::Gamepad => {
            p.add(rrect(-9.0, -5.0, 9.0, 6.0, 5.0));
            p.add(poly(&[(-5.5, 0.5), (-1.5, 0.5)]));
            p.add(poly(&[(-3.5, -1.5), (-3.5, 2.5)]));
            p.circle_filled(at(3.5, -0.5), 1.1 * k, color);
            p.circle_filled(at(6.0, 1.8), 1.1 * k, color);
        }
        Icon::Close => {
            p.add(poly(&[(-6.0, -6.0), (6.0, 6.0)]));
            p.add(poly(&[(6.0, -6.0), (-6.0, 6.0)]));
        }
        Icon::Minimize => {
            p.add(poly(&[(-6.5, 0.0), (6.5, 0.0)]));
        }
        Icon::Maximize => {
            p.add(rrect(-6.0, -6.0, 6.0, 6.0, 1.0));
        }
        Icon::Restore => {
            p.add(rrect(-6.0, -3.0, 3.0, 6.0, 1.0));
            p.add(poly(&[(-3.0, -3.0), (-3.0, -6.0), (6.0, -6.0), (6.0, 3.0), (3.0, 3.0)]));
        }
        Icon::Monitor => {
            p.add(rrect(-8.5, -7.0, 8.5, 4.5, 1.5));
            p.add(poly(&[(-4.0, 8.0), (4.0, 8.0)]));
            p.add(poly(&[(0.0, 4.5), (0.0, 8.0)]));
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![at(-5.0, -8.0), at(8.5, 0.0), at(-5.0, 8.0)], color, Stroke::NONE));
        }
        Icon::Clock => {
            p.circle_stroke(c, 8.5 * k, line);
            p.add(poly(&[(0.0, -5.0), (0.0, 0.0), (3.5, 2.0)]));
        }
        Icon::Refresh => {
            let pts = arc(c, 7.0 * k, 0.35, 5.5, 20);
            let end = *pts.last().unwrap();
            p.add(Shape::line(pts, line));
            p.add(Shape::convex_polygon(vec![end + vec2(-4.0 * k, -3.0 * k), end + vec2(3.5 * k, -3.5 * k), end + vec2(0.5 * k, 3.5 * k)], color, Stroke::NONE));
        }
        Icon::Film => {
            p.add(rrect(-9.0, -7.0, 9.0, 7.0, 1.5));
            p.add(Shape::convex_polygon(vec![at(-2.5, -3.5), at(4.0, 0.0), at(-2.5, 3.5)], color, Stroke::NONE));
        }
        Icon::Trash => {
            p.add(poly(&[(-8.0, -5.5), (8.0, -5.5)]));
            p.add(poly(&[(-3.0, -5.5), (-3.0, -8.5), (3.0, -8.5), (3.0, -5.5)]));
            p.add(closed(&[(-6.0, -5.5), (6.0, -5.5), (5.0, 8.5), (-5.0, 8.5)]));
        }
        Icon::Gear => {
            let pts: Vec<Pos2> = (0..32).map(|i| {
                let a = i as f32 / 32.0 * TAU;
                let r = if (i / 2) % 2 == 0 { 9.0 } else { 6.8 };
                at(r * a.cos(), r * a.sin())
            }).collect();
            p.add(Shape::closed_line(pts, line));
            p.circle_stroke(c, 3.0 * k, line);
        }
        Icon::SignOut => {
            p.add(poly(&[(1.0, -8.0), (-7.5, -8.0), (-7.5, 8.0), (1.0, 8.0)]));
            p.add(poly(&[(-2.0, 0.0), (8.5, 0.0)]));
            p.add(poly(&[(5.0, -3.5), (8.5, 0.0), (5.0, 3.5)]));
        }
        Icon::Flag => {
            p.add(poly(&[(-6.5, 9.0), (-6.5, -8.5)]));
            p.add(closed(&[(-6.5, -8.5), (7.5, -8.5), (4.0, -4.0), (7.5, 0.5), (-6.5, 0.5)]));
        }
        Icon::Download => {
            p.add(poly(&[(0.0, -9.0), (0.0, 3.0)]));
            p.add(poly(&[(-4.0, -1.0), (0.0, 3.0), (4.0, -1.0)]));
            p.add(poly(&[(-8.0, 4.0), (-8.0, 8.5), (8.0, 8.5), (8.0, 4.0)]));
        }
    }
}

/// A square icon button with a hairline edge (Copy link, the view toggle's halves). `danger` lights it terracotta.
pub fn icon_button(ui: &mut egui::Ui, kind: Icon, size: f32, tip: &str, danger: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let hov = resp.hovered();
    let col = if hov { if danger { RED } else { GOLD } } else { MUTED };
    let p = ui.painter();
    p.rect_filled(rect, 8.0, if hov { rgba(255, 255, 255, 10) } else { rgba(255, 255, 255, 4) });
    p.rect_stroke(rect, 8.0, Stroke::new(1.0, if hov { col } else { Color32::from_rgb(52, 52, 60) }), egui::StrokeKind::Inside);
    icon(p, rect.center(), size * 0.48, kind, col);
    let _ = pos2;
    resp.on_hover_text(tip)
}
