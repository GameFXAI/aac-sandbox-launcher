//! The frame every page sits in: the title bar (logo, Library / Community / Guild, the account menu, the window's
//! buttons), the left rail (Record, the section's pages, Friends), the inspector's panel and the window's edges.

use super::theme::*;
use super::widgets::{EDGE, close_popover, menu_item, popover, rail_item, record_button, top_tab, win_button};
use super::{Action, App, Page, RAIL_W, Section};
use eframe::egui::{self, Align2, Color32, CursorIcon, Painter, Rect, ResizeDirection, Sense, Shape, Stroke, ViewportCommand, pos2, vec2};

/// The logo: three peaks in warm white with an amber spark (drawn, no picture file).
pub fn logo(p: &Painter, c: egui::Pos2, s: f32) {
    let k = s / 24.0;
    let at = |x: f32, y: f32| c + vec2(x * k, y * k);
    let pts = vec![at(-12.0, 7.0), at(-5.0, -6.0), at(0.0, 2.0), at(5.0, -6.0), at(10.0, 4.0)];
    p.add(Shape::line(pts, Stroke::new(3.2 * k, INK)));
    p.circle_filled(at(13.5, 6.0), 2.4 * k, GOLD);
    for i in 1..=3 {
        p.circle_filled(at(13.5, 6.0), (2.4 + i as f32 * 1.6) * k, gold_a(24 - i * 6));
    }
}

/// The inspector's panel: a touch lighter than the window, with a hairline on its left.
pub fn inspector_panel(p: &Painter, r: Rect) {
    let panel = Rect::from_min_max(pos2(r.left(), r.top() + 2.0), r.max);
    p.rect_filled(panel, egui::CornerRadius { nw: 14, ne: 0, sw: 0, se: 0 }, Color32::from_rgb(16, 16, 19));
    p.rect_stroke(panel, egui::CornerRadius { nw: 14, ne: 0, sw: 0, se: 0 }, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
}

/// The borderless window's edges: the cursor changes over them and a drag resizes the window.
pub fn resize_edges(ui: &mut egui::Ui, full: Rect) {
    if ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let e = 5.0;
    let (l, r, t, b) = (full.left(), full.right(), full.top(), full.bottom());
    let zones = [
        (Rect::from_min_max(pos2(l, t), pos2(l + e * 2.0, t + e * 2.0)), ResizeDirection::NorthWest, CursorIcon::ResizeNorthWest),
        (Rect::from_min_max(pos2(r - e * 2.0, t), pos2(r, t + e * 2.0)), ResizeDirection::NorthEast, CursorIcon::ResizeNorthEast),
        (Rect::from_min_max(pos2(l, b - e * 2.0), pos2(l + e * 2.0, b)), ResizeDirection::SouthWest, CursorIcon::ResizeSouthWest),
        (Rect::from_min_max(pos2(r - e * 2.0, b - e * 2.0), pos2(r, b)), ResizeDirection::SouthEast, CursorIcon::ResizeSouthEast),
        (Rect::from_min_max(pos2(l + e * 2.0, t), pos2(r - e * 2.0, t + e)), ResizeDirection::North, CursorIcon::ResizeNorth),
        (Rect::from_min_max(pos2(l + e * 2.0, b - e), pos2(r - e * 2.0, b)), ResizeDirection::South, CursorIcon::ResizeSouth),
        (Rect::from_min_max(pos2(l, t + e * 2.0), pos2(l + e, b - e * 2.0)), ResizeDirection::West, CursorIcon::ResizeWest),
        (Rect::from_min_max(pos2(r - e, t + e * 2.0), pos2(r, b - e * 2.0)), ResizeDirection::East, CursorIcon::ResizeEast),
    ];
    for (i, (zone, dir, cursor)) in zones.into_iter().enumerate() {
        let resp = ui.interact(zone, ui.scope_id().with(("edge", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if resp.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

impl App {
    pub(in crate::gui) fn title_bar(&mut self, ui: &mut egui::Ui, rect: Rect, shell: bool, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, Color32::from_rgb(9, 9, 11));
        p.line_segment([pos2(rect.left(), rect.bottom() - 0.5), pos2(rect.right(), rect.bottom() - 0.5)], Stroke::new(1.0, LINE));
        let drag = ui.interact(rect, ui.scope_id().with("titlebar"), Sense::click_and_drag());
        let maxed = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        if drag.double_clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maxed));
        } else if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        logo(&p, pos2(rect.left() + 34.0, rect.center().y), 26.0);
        p.text(pos2(rect.left() + 60.0, rect.center().y), Align2::LEFT_CENTER, "AA Replay", bold_font(16.5), INK);

        // the sections, lined up with the page column
        if shell && self.signed_in() {
            let tabs = Rect::from_min_max(pos2(rect.left() + RAIL_W + 4.0, rect.top()), pos2(rect.right() - 320.0, rect.bottom()));
            let mut ui_tabs = ui.new_child(egui::UiBuilder::new().max_rect(tabs).layout(egui::Layout::left_to_right(egui::Align::Center)));
            ui_tabs.spacing_mut().item_spacing.x = 4.0;
            for (s, ic, label) in [(Section::Library, Icon::Folder, "Library"), (Section::Community, Icon::People, "Community"), (Section::Guild, Icon::Shield, "Guild")] {
                if top_tab(&mut ui_tabs, ic, label, self.page.section() == s, rect.height()).clicked() {
                    self.page = Page::home(s);
                }
            }
        }

        // right: the window's buttons, then the account menu
        let right = Rect::from_min_max(pos2(rect.right() - 420.0, rect.top()), rect.max);
        let mut ui_r = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        ui_r.spacing_mut().item_spacing.x = 0.0;
        if win_button(&mut ui_r, Icon::Close, rect.height()).clicked() {
            ui_r.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
        if win_button(&mut ui_r, if maxed { Icon::Restore } else { Icon::Maximize }, rect.height()).clicked() {
            ui_r.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maxed));
        }
        if win_button(&mut ui_r, Icon::Minimize, rect.height()).clicked() {
            ui_r.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        ui_r.add_space(14.0);
        self.account_button(&mut ui_r, act);
        if let Some(r) = self.update.clone() {
            ui_r.add_space(10.0);
            let g = ui_r.painter().layout_no_wrap(format!("Update to {}", r.version), medium_font(12.5), GOLD_HI);
            let (b, resp) = ui_r.allocate_exact_size(vec2(g.size().x + 22.0, 26.0), Sense::click());
            let free = !self.busy && self.child.is_none();
            bronze(ui_r.painter(), b, 13.0, if resp.hovered() { 1.6 } else { 1.0 });
            ui_r.painter().rect_stroke(b, 13.0, Stroke::new(1.0, gold_a(140)), egui::StrokeKind::Inside);
            ui_r.painter().galley(b.center() - g.size() / 2.0, g, GOLD_HI);
            let tip = if free { "A newer launcher is ready: download it and start it" } else { "Close the game first" };
            if resp.on_hover_text(tip).clicked() && free {
                *act = Some(Action::UpdateLauncher);
            }
        }
    }

    /// The account's initial (a gear when signed out) and its menu: who, the plan, Settings, the transfer, Report a
    /// problem, the log, Sign out.
    fn account_button(&mut self, ui: &mut egui::Ui, act: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(vec2(58.0, 40.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Account"));
        let id = ui.make_persistent_id("account-menu");
        let open = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let p = ui.painter().clone();
        if resp.hovered() || open {
            p.rect_filled(rect, 20.0, rgba(255, 255, 255, 10));
        }
        let c = pos2(rect.left() + 20.0, rect.center().y);
        let signed = self.signed_in();
        let name = self.account_name();
        if signed {
            avatar(&p, c, 14.0, &name);
        } else {
            p.circle_stroke(c, 14.0, Stroke::new(1.0, EDGE));
            icon(&p, c, 16.0, Icon::Gear, MUTED);
        }
        icon(&p, pos2(rect.right() - 12.0, rect.center().y), 11.0, Icon::ChevronDown, MUTED);
        let plan = self.lib.data.clone();
        let mut go = None;
        popover(ui, id, &resp, 260.0, true, |ui| {
            if signed {
                let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
                let hp = ui.painter();
                avatar(hp, pos2(head.left() + 22.0, head.center().y), 16.0, &name);
                hp.text(pos2(head.left() + 48.0, head.top() + 10.0), Align2::LEFT_TOP, &name, bold_font(14.5), INK);
                let line = match &plan {
                    Some(l) => super::screens::library::plan_words(l).0,
                    None => "Signed in".into(),
                };
                hp.text(pos2(head.left() + 48.0, head.top() + 30.0), Align2::LEFT_TOP, line, text_font(12.5), MUTED);
                rule(hp, head.left() + 6.0, head.right() - 6.0, head.bottom() - 1.0);
                ui.add_space(4.0);
            }
            if menu_item(ui, Some(Icon::Gear), "Settings", self.page == Page::Settings, false).clicked() {
                go = Some(Page::Settings);
            }
            if menu_item(ui, Some(Icon::Download), "Bring AA Classic settings", self.page == Page::Transfer, false).clicked() {
                *act = Some(Action::OpenTransfer);
                close_popover(ui, id);
            }
            if menu_item(ui, Some(Icon::Flag), "Report a problem", false, false).clicked() {
                *act = Some(Action::Report);
                close_popover(ui, id);
            }
            if menu_item(ui, Some(Icon::List), "Log", self.page == Page::Log, false).clicked() {
                go = Some(Page::Log);
            }
            if signed && self.saved.is_some() && menu_item(ui, Some(Icon::SignOut), "Sign out", false, true).clicked() {
                *act = Some(Action::SignOut);
                close_popover(ui, id);
            }
            if go.is_some() {
                close_popover(ui, id);
            }
        });
        if let Some(pg) = go {
            self.page = pg;
        }
    }

    pub(in crate::gui) fn rail(&mut self, ui: &mut egui::Ui, rect: Rect, act: &mut Option<Action>) {
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, Color32::from_rgb(12, 12, 14));
        let inner = rect.shrink2(vec2(14.0, 0.0));
        let w = inner.width();
        let mut ui_r = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(inner.left(), rect.top() + 18.0), inner.max)).layout(egui::Layout::top_down(egui::Align::Min)));
        ui_r.spacing_mut().item_spacing.y = 4.0;

        // Record: the recorder's state, and its controls in a menu
        let (live, chip, chip_col) = self.record_chip();
        let id = ui_r.make_persistent_id("record-menu");
        let open = ui_r.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let resp = record_button(&mut ui_r, w, live, chip, chip_col, open).on_hover_text("Recording AA Classic: your own fights become captures here");
        popover(&ui_r, id, &resp, 400.0, false, |ui| self.record_menu(ui, act));
        ui_r.add_space(16.0);

        let section = self.page.section();
        for &pg in Page::rail(section) {
            if rail_item(&mut ui_r, pg.icon(), pg.label(section), self.page == pg, w).clicked() {
                self.page = pg;
            }
        }

        // Friends, at the bottom
        if section != Section::Account {
            let top = rect.bottom() - 132.0;
            rule(&p, inner.left() + 4.0, inner.right() - 4.0, top);
            p.text(pos2(inner.left() + 8.0, top + 18.0), Align2::LEFT_TOP, "Friends", bold_font(15.0), INK);
            let g = p.layout(
                "Your in-game friends and what they are doing show here once the server shares them.".into(),
                text_font(12.5),
                MUTED,
                w - 16.0,
            );
            p.galley(pos2(inner.left() + 8.0, top + 46.0), g, MUTED);
        }
    }
}
