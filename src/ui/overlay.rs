//! The overlays: the status overlay and the tile overlays (extra windows that stay with GSPro),
//! what they show and how they are drawn, and the *Overlay* page with its previews and
//! arrangements.

use super::*;

impl App {
    /// Always-on-top, click-through status panel for playing GSPro in full screen.
    pub(super) fn overlay(&mut self, ctx: &egui::Context) {
        self.status_rect = None;
        let Some((ax, ay)) = self.saved.overlay.anchor().filter(|_| self.show_overlay) else {
            self.moving = self.moving.take().filter(|m| m.tile.is_some());
            self.passthrough.remove(&None);
            return;
        };
        let Some(screen) = self.screen_size(ctx) else {
            return;
        };
        let style = &self.saved.overlay_style;
        let content = OverlayContent::live(style, self.saved.metric, self.saved.meters);
        // Measured in the overlay's own window last frame, at least the size measured here.
        let base = content.size(ctx).max(self.overlay_base.get());
        // The overlay window's size and place go to the system in native points (without the
        // zoom of egui's points, which the drawing below takes into account), within the part of
        // the screen not covered by the menu bar and the Dock.
        let zoom = ctx.zoom_factor();
        // A transparent border around the panel, so rounding the window to whole pixels never
        // cuts its edge.
        let size = (base + vec2(2.0, 2.0) * OVERLAY_BORDER) * style.scale;
        #[cfg(target_os = "macos")]
        let visible = mac::visible_frame();
        #[cfg(not(target_os = "macos"))]
        let visible: Option<Rect> = None;
        let visible = visible.unwrap_or(Rect::from_min_size(egui::Pos2::ZERO, screen * zoom));
        let moving = self.moving.as_ref().filter(|m| m.tile.is_none());
        let pos = moving.and_then(|m| m.at).unwrap_or_else(|| overlay_origin(visible, size, (ax, ay), self.saved.overlay_spot));
        // On macOS the window is placed through AppKit (below): egui takes the size and place
        // of this window in native points but reports them with the zoom, so the builder only
        // gives the first ones and then stays the same.
        #[cfg(target_os = "macos")]
        let (pos, size_asked) = *self.overlay_first.get_or_insert((pos, size));
        // Elsewhere through egui, in the overlay pass's points (zoomed by the overlay's size).
        #[cfg(not(target_os = "macos"))]
        let size_asked = size / style.scale;
        #[cfg(not(target_os = "macos"))]
        let pos = pos / style.scale;
        let grabbable = moving.is_some();
        let builder = egui::ViewportBuilder::default()
            .with_title(OVERLAY_TITLE)
            .with_position(pos)
            .with_inner_size(size_asked)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_mouse_passthrough(!grabbable)
            .with_active(false)
            .with_taskbar(false);
        let capture = &mut self.capture;
        let overlay_base = &self.overlay_base;
        let user_scale = style.scale;
        let pos_native = moving.and_then(|m| m.at).unwrap_or_else(|| overlay_origin(visible, size, (ax, ay), self.saved.overlay_spot));
        self.status_rect = Some(Rect::from_min_size(pos_native, size));
        let mut moving = self.moving.as_mut().filter(|m| m.tile.is_none());
        let mut dropped = None;
        // The overlay's pass takes the zoom from the options when it begins: the overlay's size
        // there, the main window's back right after (no repaint is triggered either way).
        ctx.options_mut(|o| o.zoom_factor = user_scale);
        let covered = self.all_covered;
        ctx.show_viewport_immediate(overlay_id(), builder, |ui, _class| {
            // Only kept open (see `App::logic`).
            if covered {
                return;
            }
            #[cfg(target_os = "macos")]
            mac::float_over_fullscreen(ui.ctx(), OVERLAY_TITLE, grabbable);
            // Measured here, in the overlay's own window (the next frame sizes the window by it).
            let own = content.size(ui.ctx());
            overlay_base.set(own);
            #[cfg(target_os = "macos")]
            mac::place_window(OVERLAY_TITLE, pos_native, size);

            // This window's pass runs with the overlay's size as its zoom (set above), so the
            // overlay is drawn at its base size: no scaling of shapes or layers needed.
            let rect = Rect::from_min_size(pos2(OVERLAY_BORDER, OVERLAY_BORDER), own);
            overlay_ui(&ui.ctx().layer_painter(overlay_layer()).with_clip_rect(rect.expand(OVERLAY_BORDER)), rect, &content);
            if let Some(m) = moving.as_deref_mut() {
                dropped = drag_overlay(ui, rect, m, pos_native, user_scale);
            }
            capture.save_from(ui.ctx());
        });
        ctx.options_mut(|o| o.zoom_factor = zoom);
        self.set_passthrough(ctx, None, !grabbable);
        if let Some(at) = dropped {
            let spot = overlay_spot(visible, size, at);
            self.moving = None;
            self.change_overlays(|s| s.overlay_spot = Some(spot));
        }
    }

    /// The main window's screen size, or the last one known (see `screen_size`).
    fn screen_size(&mut self, ctx: &egui::Context) -> Option<egui::Vec2> {
        if let Some(size) = ctx.input(|i| i.viewport().monitor_size) {
            self.screen_size = Some(size);
        }
        self.screen_size
    }

    /// Lets clicks go through the status (`None`) or a tile overlay, or not while it is moved;
    /// told to the window when it changes.
    fn set_passthrough(&mut self, ctx: &egui::Context, which: Option<usize>, on: bool) {
        if self.passthrough.get(&which) != Some(&on) {
            let id = which.map_or_else(overlay_id, tile_overlay_id);
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::MousePassthrough(on));
            self.passthrough.insert(which, on);
        }
    }

    /// Changes the overlays (applied and saved at once), in the arrangement in use, too.
    pub(super) fn change_overlays(&mut self, change: impl Fn(&mut Settings)) {
        let _ = self.change_settings(|s| {
            change(s);
            s.keep_arrangement();
        });
    }

    /// Shows the overlays of the arrangement `choose` returns, after the changes it makes (and
    /// saves that it is in use).
    pub(super) fn use_arrangement(&mut self, choose: impl Fn(&mut Settings) -> usize) {
        self.change_overlays(|s| {
            let i = choose(s);
            s.use_arrangement(i);
        });
        self.moving = None;
        self.overlay_tab = self.overlay_tab.filter(|&k| k < self.saved.tile_overlays.len());
    }

    /// The arrangement's name as shown (the first one's translated while unchanged).
    pub(super) fn arrangement_name(&self, i: usize) -> String {
        let name = self.saved.arrangements.get(i).map_or(DEFAULT_ARRANGEMENT, |a| a.name.as_str());
        if name == DEFAULT_ARRANGEMENT { tr(DEFAULT_ARRANGEMENT).to_string() } else { name.to_string() }
    }

    /// Choosing, renaming, adding and deleting arrangements: the menu of `resp`. `full` adds
    /// renaming and the rest (the Overlay page), else only choosing (the Play page).
    pub(super) fn arrangement_menu(&mut self, resp: &egui::Response, full: bool) {
        let mut chosen = None;
        let mut renamed = None;
        let mut added = false;
        egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for i in 0..self.saved.arrangements.len() {
                if ui.selectable_label(i == self.saved.arrangement, self.arrangement_name(i)).clicked() {
                    chosen = Some(i);
                    ui.close();
                }
            }
            if !full {
                return;
            }
            ui.separator();
            let mut name = self.arrangement_name(self.saved.arrangement);
            if ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("Name")).desired_width(170.0)).on_hover_text(tr("Name of the arrangement")).changed() {
                renamed = Some(name);
            }
            if ui.button(tr("Save as a new arrangement")).on_hover_text(tr("A copy of the overlays as they are, to change from there")).clicked() {
                added = true;
                ui.close();
            }
            if self.saved.arrangements.len() > 1 && ui.button(RichText::new(tr("Delete arrangement")).color(RED)).clicked() {
                self.confirm_delete_arrangement = Some(self.saved.arrangement);
                ui.close();
            }
        });
        if let Some(name) = renamed {
            self.change_overlays(|s| {
                let i = s.arrangement;
                s.arrangements[i].name.clone_from(&name);
            });
        }
        if added {
            let name = tr("Arrangement {n}").replace("{n}", &(self.saved.arrangements.len() + 1).to_string());
            self.use_arrangement(|s| {
                let copy = Arrangement { name: name.clone(), ..s.arrangements[s.arrangement].clone() };
                s.arrangements.push(copy);
                s.arrangements.len() - 1
            });
        }
        if let Some(i) = chosen {
            self.use_arrangement(|_| i);
        }
    }

    /// "Delete the arrangement?" while asked.
    fn confirm_delete_arrangement(&mut self, ctx: &egui::Context) {
        let Some(i) = self.confirm_delete_arrangement else { return };
        let title = tr("Delete the arrangement “{name}”?").replace("{name}", &self.arrangement_name(i));
        let Some(delete) = confirm(ctx, "delete-arrangement", &title, tr("Its overlays go with it."), tr("DELETE")) else { return };
        self.confirm_delete_arrangement = None;
        if delete && i < self.saved.arrangements.len() && self.saved.arrangements.len() > 1 {
            self.use_arrangement(|s| {
                s.arrangements.remove(i);
                // Shown without writing the deleted one's overlays into another.
                s.arrangement = s.arrangement.min(s.arrangements.len() - 1);
                s.arrangement
            });
        }
    }

    /// Always-on-top, click-through windows with one tile of the shots explorer each. Overlays at
    /// the same place (not placed by hand) stack: each under the one before (above it at the bottom
    /// of the screen), the status first.
    pub(super) fn tile_overlays(&mut self, ctx: &egui::Context) {
        let tiles = self.saved.tile_overlays.clone();
        self.moving = self.moving.take().filter(|m| m.tile.is_none_or(|k| tiles.get(k).is_some_and(|t| t.pos != OverlayPos::Off)));
        if !self.show_overlay {
            return;
        }
        let Some(screen) = self.screen_size(ctx) else {
            return;
        };
        let zoom = ctx.zoom_factor();
        #[cfg(target_os = "macos")]
        let visible = mac::visible_frame();
        #[cfg(not(target_os = "macos"))]
        let visible: Option<Rect> = None;
        let visible = visible.unwrap_or(Rect::from_min_size(egui::Pos2::ZERO, screen * zoom));
        // The last overlay at each of the nine places.
        let mut taken: HashMap<OverlayPos, Rect> = HashMap::new();
        if let Some(status) = self.status_rect.filter(|_| self.saved.overlay_spot.is_none()) {
            taken.insert(self.saved.overlay, status);
        }
        let (shots, club) = state::with(|s| (s.session.clone(), s.gspro_club.clone()));
        for (k, cfg) in tiles.iter().enumerate() {
            let Some((ax, ay)) = cfg.pos.anchor() else {
                self.passthrough.remove(&Some(k));
                continue;
            };
            let size = vec2(cfg.width, cfg.height()) + vec2(2.0, 2.0) * OVERLAY_BORDER;
            let mut pos = overlay_origin(visible, size, (ax, ay), cfg.spot);
            if cfg.spot.is_none() {
                if let Some(before) = taken.get(&cfg.pos) {
                    pos.y = if ay < 1.0 { before.bottom() + 8.0 } else { before.top() - 8.0 - size.y };
                }
                taken.insert(cfg.pos, Rect::from_min_size(pos, size));
            }
            let mut moving = self.moving.as_mut().filter(|m| m.tile == Some(k));
            if let Some(at) = moving.as_ref().and_then(|m| m.at) {
                pos = at;
            }
            let grabbable = moving.is_some();
            #[cfg(target_os = "macos")]
            let (first_pos, size_asked) = *self.tile_overlay_first.entry(k).or_insert((pos, size));
            #[cfg(not(target_os = "macos"))]
            let (first_pos, size_asked) = (pos, size);
            let title = tile_overlay_title(k);
            let builder = egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_position(first_pos)
                .with_inner_size(size_asked)
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_maximize_button(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_mouse_passthrough(!grabbable)
                .with_active(false)
                .with_taskbar(false);
            let units = (self.saved.metric, self.saved.meters);
            let gspro = self.saved.gspro_results;
            let explorer = &mut self.explorer;
            let mut dropped = None;
            // Drawn at its own size, whatever the main window's zoom (see `overlay`).
            ctx.options_mut(|o| o.zoom_factor = 1.0);
            let covered = self.all_covered;
            ctx.show_viewport_immediate(tile_overlay_id(k), builder, |ui, _class| {
                // Only kept open (see `App::logic`).
                if covered {
                    return;
                }
                #[cfg(target_os = "macos")]
                {
                    mac::float_over_fullscreen(ui.ctx(), &title, grabbable);
                    mac::place_window(&title, pos, size);
                }
                let rect = Rect::from_min_size(pos2(OVERLAY_BORDER, OVERLAY_BORDER), vec2(cfg.width, cfg.height()));
                explorer.overlay_tile(ui, rect, k, cfg, &shots, club.as_deref(), units, gspro);
                if let Some(m) = moving.as_deref_mut() {
                    dropped = drag_overlay(ui, rect, m, pos, 1.0);
                }
            });
            ctx.options_mut(|o| o.zoom_factor = zoom);
            self.set_passthrough(ctx, Some(k), !grabbable);
            if let Some(at) = dropped {
                let spot = overlay_spot(visible, size, at);
                self.moving = None;
                self.change_overlays(|s| s.tile_overlays[k].spot = Some(spot));
            }
        }
    }

    /// Overlay position, size and contents with a preview; changes apply and are saved at once.
    pub(super) fn overlay_page(&mut self, ui: &mut egui::Ui) {
        self.confirm_delete_arrangement(ui.ctx());
        if let Some(k) = self.overlay_tab.filter(|&k| k < self.saved.tile_overlays.len()) {
            return self.tile_overlay_page(ui, k);
        }
        self.overlay_tab = None;
        let mut pos = self.saved.overlay;
        let mut spot = self.saved.overlay_spot;
        let mut style = self.saved.overlay_style.clone();
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 254.0), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let left = Rect::from_min_size(area.min, vec2(col_w, area.height()));
        let right = Rect::from_min_size(pos2(area.left() + col_w + 10.0, area.top()), vec2(col_w, area.height()));

        let title = self.arrangement_name(self.saved.arrangement).to_uppercase();
        card(ui, left, &title, Icon::Layout, |ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                setting_label(ui, tr("Position"), 72.0);
                if overlay_picker(ui, &mut pos, spot.is_some()) {
                    spot = None;
                }
                ui.add_space(12.0);
                place_text(ui, pos, spot.is_some(), tr("On top of GSPro, also in full screen."), &mut self.moving, None);
            });
            setting_row(ui, tr("Size"), |ui| {
                segments(ui, &mut style.scale, &[(0.75, "75 %"), (1.0, "100 %"), (1.25, "125 %"), (1.5, "150 %")]);
            });
            setting_row(ui, tr("Lines"), |ui| {
                segments(ui, &mut style.one_line, &[(false, tr("Two lines")), (true, tr("One line"))]);
            });
            setting_row(ui, tr("Background"), |ui| {
                segments(ui, &mut style.opacity, &[(0.7, "70 %"), (0.85, "85 %"), (1.0, tr("Solid"))]);
            });
        });
        self.overlay_card_head(ui, left, &title);

        card(ui, right, tr("CONTENTS"), Icon::List, |ui| {
            ui.label(RichText::new(tr("Click a tile in the preview to choose what it shows, drag it to move it.")).size(13.0).color(SOFT));
            ui.label(RichText::new(tr("+ at the end of the tiles adds a column. The status always comes first.")).size(12.0).color(MUTED));
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if secondary_button(ui, tr("Reset"), style.layout != OverlayItem::default_layout())
                    .on_hover_text(tr("Back to the tiles as they came"))
                    .clicked()
                {
                    style.layout = OverlayItem::default_layout();
                }
            });

        });

        // Below the cards, down to the page's bottom margin (the cards move the layout cursor).
        let preview = Rect::from_min_max(pos2(area.left(), area.bottom() + 10.0), pos2(area.right(), ui.max_rect().bottom()));
        self.overlay_preview(ui, preview, pos, &mut style);

        if pos != self.saved.overlay || spot != self.saved.overlay_spot || style != self.saved.overlay_style {
            self.change_overlays(|s| {
                s.overlay = pos;
                s.overlay_spot = spot;
                s.overlay_style.clone_from(&style);
            });
        }
    }

    /// The tile overlay's position, size and tile with a preview; changes apply and are saved at once.
    fn tile_overlay_page(&mut self, ui: &mut egui::Ui, k: usize) {
        let mut cfg = self.saved.tile_overlays[k].clone();
        let mut remove = false;
        // One card with everything (its tabs where the status overlay's are), the preview next to it.
        let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let left = Rect::from_min_size(area.min, vec2(col_w, area.height()));
        let right = Rect::from_min_size(pos2(area.left() + col_w + 10.0, area.top()), vec2(col_w, area.height()));

        let title = self.arrangement_name(self.saved.arrangement).to_uppercase();
        card(ui, left, &title, Icon::Layout, |ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                setting_label(ui, tr("Position"), 72.0);
                if overlay_picker(ui, &mut cfg.pos, cfg.spot.is_some()) {
                    cfg.spot = None;
                }
                ui.add_space(12.0);
                place_text(ui, cfg.pos, cfg.spot.is_some(), tr("Under the overlays at its place."), &mut self.moving, Some(k));
            });
            setting_row(ui, tr("Size"), |ui| {
                let [s, m, l] = TileOverlay::WIDTHS;
                segments(ui, &mut cfg.width, &[(s, tr("Small")), (m, tr("Medium")), (l, tr("Large"))]);
            });
            setting_row(ui, tr("Background"), |ui| {
                segments(ui, &mut cfg.opacity, &[(0.7, "70 %"), (0.85, "85 %"), (1.0, tr("Solid"))]);
            });
            explorer::tile_options(ui, &mut cfg.tile, self.saved.metric, self.saved.meters, |ui| {
                ui.add_space(4.0);
                let resp = ui.add(egui::Button::new(RichText::new(tr("Remove tile")).size(13.0).color(RED)).frame(false));
                if resp.on_hover_text(tr("Takes this tile off the screen and out of the arrangement")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    remove = true;
                }
            });
            setting_row(ui, tr("Shots"), |ui| {
                segments(ui, &mut cfg.club_only, &[(false, tr("This session")), (true, tr("Club in GSPro"))]);
            });
        });
        self.overlay_card_head(ui, left, &title);
        self.tile_overlay_preview(ui, right, &cfg);

        if remove {
            self.change_overlays(|s| {
                s.tile_overlays.remove(k);
            });
            self.moving = None;
            self.overlay_tab = k.checked_sub(1).or((!self.saved.tile_overlays.is_empty()).then_some(0));
        } else if cfg != self.saved.tile_overlays[k] {
            self.change_overlays(|s| s.tile_overlays[k].clone_from(&cfg));
        }
    }

    /// The left card's head on the Overlay page: the arrangement's name opens its menu, the tabs
    /// pick the overlay edited, *+* adds a tile.
    fn overlay_card_head(&mut self, ui: &mut egui::Ui, card: Rect, title: &str) {
        let w = ui.painter().layout_no_wrap(title.to_string(), cond(15.0), LABEL).size().x + title.chars().count() as f32 * 1.5;
        let head = Rect::from_min_max(pos2(card.left() + 14.0, card.top() + 6.0), pos2(card.left() + 58.0 + w, card.top() + 36.0));
        let resp = ui.interact(head, ui.id().with("arrangement"), Sense::click());
        let c = pos2(head.right() - 4.0, card.top() + 21.0);
        let color = if resp.hovered() { WHITE } else { LABEL };
        ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], color, Stroke::NONE));
        let resp = resp.on_hover_text(tr("Arrangement: click to switch, rename or save as a new one")).on_hover_cursor(egui::CursorIcon::PointingHand);
        self.arrangement_menu(&resp, true);
        let count = self.saved.tile_overlays.len();
        if overlay_tabs(ui, card, &mut self.overlay_tab, count) {
            self.change_overlays(|s| s.tile_overlays.push(TileOverlay::default()));
            self.overlay_tab = Some(count);
        }
    }

    /// The tile overlay on the GSPro-like backdrop, scaled to fit, with the session's shots (or
    /// samples while there are none).
    fn tile_overlay_preview(&mut self, ui: &mut egui::Ui, area: Rect, cfg: &TileOverlay) {
        let p = ui.painter();
        backdrop(p, area);
        let anchor = cfg.pos.anchor();
        let label_right = anchor.is_some_and(|(ax, ay)| ay > 0.5 && ax < 1.0);
        let (label_pos, label_align) = if label_right {
            (pos2(area.right() - 12.0, area.bottom() - 14.0), egui::Align::Max)
        } else {
            (pos2(area.left() + 12.0, area.bottom() - 14.0), egui::Align::Min)
        };
        caps_text(p, label_pos, label_align, tr("PREVIEW"), 14.0, SOFT);
        let Some((ax, ay)) = anchor else {
            p.text(area.center(), Align2::CENTER_CENTER, tr("Overlay off"), FontId::proportional(14.0), SOFT);
            return;
        };
        // Placed by hand: there in the preview, too.
        let [ax, ay] = cfg.spot.unwrap_or([ax, ay]);
        let (mut shots, club) = state::with(|s| (s.session.clone(), s.gspro_club.clone()));
        if shots.is_empty() {
            shots = self.sample_shots.get_or_init(engine::demo_session).clone();
        }
        let club = club.or_else(|| shots.last().and_then(|s| s.club_code.clone()));
        let size = vec2(cfg.width, cfg.height());
        let pad = 9.0;
        let room = area.size() - vec2(2.0 * pad, 2.0 * pad);
        let scale = (room.x / size.x).min(room.y / size.y).min(1.0);
        let free = (room - size * scale).max(egui::Vec2::ZERO);
        let origin = area.min + vec2(pad + free.x * ax, pad + free.y * ay);
        // Scaled on its own layer, like the status overlay's preview.
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("tile-overlay-preview"));
        ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::new(origin.to_vec2(), scale));
        let rect = Rect::from_min_size(egui::Pos2::ZERO, size);
        let clip = Rect::from_min_max(((area.min - origin) / scale).to_pos2(), ((area.max - origin) / scale).to_pos2());
        let mut tile_ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
        tile_ui.set_clip_rect(clip);
        let units = (self.saved.metric, self.saved.meters);
        self.explorer.overlay_tile(&mut tile_ui, rect, 500, cfg, &shots, club.as_deref(), units, self.saved.gspro_results);
    }

    /// The overlay with a sample shot on a GSPro-like backdrop, at its size and position.
    fn overlay_preview(&self, ui: &egui::Ui, area: Rect, pos: OverlayPos, style: &mut OverlayStyle) {
        let p = ui.painter();
        backdrop(p, area);
        let anchor = pos.anchor();
        // The label goes to the bottom corner the overlay leaves free.
        let label_right = anchor.is_some_and(|(ax, ay)| ay > 0.5 && ax < 1.0);
        let (label_pos, label_align) = if label_right {
            (pos2(area.right() - 12.0, area.bottom() - 14.0), egui::Align::Max)
        } else {
            (pos2(area.left() + 12.0, area.bottom() - 14.0), egui::Align::Min)
        };
        caps_text(p, label_pos, label_align, tr("PREVIEW"), 14.0, SOFT);
        let Some((ax, ay)) = anchor else {
            p.text(area.center(), Align2::CENTER_CENTER, tr("Overlay off"), FontId::proportional(14.0), SOFT);
            return;
        };
        let [ax, ay] = self.saved.overlay_spot.unwrap_or([ax, ay]);
        let ready = (R10Status::Ready, "", true);
        let pin = Some(pin_text(139.0, self.saved.meters));
        let mut content = OverlayContent::new(style, self.saved.metric, ready, Some("I7"), Some(82), Some(&self.preview_shot), pin);
        content.edit = true;
        let size = content.size(ui.ctx());
        let pad = 9.0;
        let room = area.size() - vec2(2.0 * pad, 2.0 * pad);
        // At its size, or smaller when it doesn't fit (very long single lines).
        let scale = style.scale.min(room.x / size.x).min(room.y / size.y);
        let free = (room - size * scale).max(egui::Vec2::ZERO);
        let origin = area.min + vec2(pad + free.x * ax, pad + free.y * ay);
        // Scaled on its own layer, like the overlay window; clipped to the preview.
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("overlay-preview"));
        ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::new(origin.to_vec2(), scale));
        let clip = Rect::from_min_max(((area.min - origin) / scale).to_pos2(), ((area.max - origin) / scale).to_pos2());
        let tiles = overlay_ui(&ui.ctx().layer_painter(layer).with_clip_rect(clip), Rect::from_min_size(egui::Pos2::ZERO, size), &content);

        // A click on a tile picks what it shows; dragging it onto another tile swaps the two (the
        // status stays where it is).
        let to_page = |r: Rect| Rect::from_min_max(origin + r.min.to_vec2() * scale, origin + r.max.to_vec2() * scale);
        let tiles: Vec<(usize, usize, Rect)> =
            tiles.into_iter().filter(|(k, row, _)| (*k, *row) != (0, 0)).map(|(k, row, r)| (k, row, to_page(r).intersect(area))).collect();
        self.overlay_tile_rects.borrow_mut().clone_from(&tiles);
        let pointer = ui.ctx().pointer_interact_pos();
        let mut swap: Option<((usize, usize), (usize, usize))> = None;
        let mut insert: Option<((usize, usize), usize)> = None;
        // The lines between the columns (and after the last one): (place in the layout, x, top, bottom).
        let (top, bottom) = tiles.iter().fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t.2.top()), b.max(t.2.bottom())));
        let mut lines: Vec<(usize, f32)> = Vec::new();
        for &(k, _, r) in &tiles {
            if k > 0 && !lines.iter().any(|l| l.0 == k) {
                lines.push((k, r.left()));
            }
        }
        if let Some(&(k, _, _)) = tiles.iter().max_by_key(|t| t.0) {
            let right = tiles.iter().filter(|t| t.0 == k).map(|t| t.2.right()).fold(f32::MIN, f32::max);
            lines.push((k + 1, right));
        }
        let line_at = |at: egui::Pos2| {
            lines.iter().copied().filter(|l| (l.1 - at.x).abs() <= 7.0 && at.y >= top && at.y <= bottom).min_by(|a, b| (a.1 - at.x).abs().total_cmp(&(b.1 - at.x).abs()))
        };
        for &(k, row, r) in &tiles {
            let resp = ui.interact(r, ui.id().with(("overlay-tile", k, row)), Sense::click_and_drag());
            // Onto a line between two columns: a new column there; else onto a tile: swap.
            let on_line = pointer.and_then(line_at);
            let target = pointer
                .filter(|_| on_line.is_none())
                .and_then(|at| tiles.iter().find(|t| t.2.contains(at)))
                .filter(|t| (t.0, t.1) != (k, row));
            if resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("overlay-drag")));
                fg.rect_filled(r, 4.0, Color32::from_black_alpha(90));
                if let Some(t) = target {
                    fg.rect_stroke(t.2.shrink(1.0), 4.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
                }
                if let Some((_, x)) = on_line {
                    fg.line_segment([pos2(x, top + 2.0), pos2(x, bottom - 2.0)], Stroke::new(3.0, ACCENT));
                }
                if let Some(at) = pointer {
                    let name = style.layout.get(k).and_then(|c| c[row]).map_or(tr("Empty"), overlay_item_name);
                    let galley = fg.layout_no_wrap(name.to_string(), FontId::proportional(12.0), WHITE);
                    let label = Rect::from_min_size(at + vec2(12.0, 8.0), galley.size() + vec2(14.0, 6.0));
                    fg.rect(label, 5.0, BUTTON, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                    fg.galley(label.min + vec2(7.0, 3.0), galley, WHITE);
                }
            } else if resp.hovered() {
                ui.painter().rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.0, ICON), StrokeKind::Inside);
            }
            if resp.drag_stopped() {
                if let Some((at, _)) = on_line {
                    insert = Some(((k, row), at));
                } else if let Some(t) = target {
                    swap = Some(((k, row), (t.0, t.1)));
                }
            }
            let resp = resp.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(tr("Click to choose what this tile shows, drag to move it"));
            egui::Popup::menu(&resp).show(|ui| {
                let current = style.layout.get(k).and_then(|c| c[row]);
                for item in OverlayItem::CHOICES {
                    if item.is_graphic() && k == 0 {
                        continue;
                    }
                    if item == OverlayItem::ImpactSide {
                        ui.separator();
                    }
                    if ui.selectable_label(current == Some(item), overlay_item_name(item)).clicked() {
                        set_tile(&mut style.layout, k, row, Some(item));
                    }
                }
                ui.separator();
                if ui.selectable_label(current.is_none(), tr("Empty")).clicked() {
                    set_tile(&mut style.layout, k, row, None);
                }
                // A value may fill its column (not next to the status, not a graphic).
                if k > 0
                    && let Some(item) = current.filter(|i| !i.is_graphic())
                {
                    let tall = style.layout[k] == [Some(item), Some(item)];
                    if ui.selectable_label(tall, tr("Double height")).clicked() {
                        style.layout[k] = if tall { [Some(item), None] } else { [Some(item), Some(item)] };
                    }
                }
                if k > 0 && ui.button(RichText::new(tr("Remove column")).color(RED)).clicked() {
                    style.layout.remove(k);
                }
            });
        }
        // + right after the last column adds one.
        if let Some(right) = tiles.iter().map(|t| t.2.right()).reduce(f32::max) {
            let top = tiles.iter().map(|t| t.2.top()).fold(f32::MAX, f32::min);
            let bottom = tiles.iter().map(|t| t.2.bottom()).fold(f32::MIN, f32::max);
            let r = Rect::from_center_size(pos2(right, f32::midpoint(top, bottom)), vec2(20.0, 20.0));
            let add = ui.interact(r, ui.id().with("overlay-add-column"), Sense::click());
            let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("overlay-add")));
            fg.circle(r.center(), 9.5, if add.hovered() { ACCENT } else { BUTTON }, Stroke::new(1.0, if add.hovered() { ACCENT_HOVER } else { BUTTON_LINE }));
            fg.line_segment([r.center() - vec2(4.5, 0.0), r.center() + vec2(4.5, 0.0)], Stroke::new(1.6, WHITE));
            fg.line_segment([r.center() - vec2(0.0, 4.5), r.center() + vec2(0.0, 4.5)], Stroke::new(1.6, WHITE));
            if add.on_hover_text(tr("Add an empty column at the end")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                style.layout.push([None, None]);
            }
        }
        if let Some((a, b)) = swap {
            swap_tiles(&mut style.layout, a, b);
        }
        if let Some((from, at)) = insert {
            insert_tile(&mut style.layout, from, at);
        }
    }
}

pub(super) fn overlay_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("overlay")
}

fn tile_overlay_id(k: usize) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("tile-overlay", k))
}

/// Window title of tile overlay `k` (AppKit finds the window by it).
fn tile_overlay_title(k: usize) -> String {
    format!("{TILE_OVERLAY_TITLE} {}", k + 1)
}

/// Layer the overlay draws on, scaled down by the main window's zoom.
fn overlay_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new("overlay-content"))
}

/// Display name of an overlay position.
fn overlay_name(pos: OverlayPos) -> &'static str {
    match pos {
        OverlayPos::Off => tr("No overlay"),
        OverlayPos::TopLeft => tr("Top left"),
        OverlayPos::Top => tr("Top"),
        OverlayPos::TopRight => tr("Top right"),
        OverlayPos::Left => tr("Left"),
        OverlayPos::Right => tr("Right"),
        OverlayPos::BottomLeft => tr("Bottom left"),
        OverlayPos::Bottom => tr("Bottom"),
        OverlayPos::BottomRight => tr("Bottom right"),
    }
}

/// Headline status and its colour, shared by the status panel and the overlay.
pub(super) fn status_word(r10: R10Status, gs_ok: bool) -> (Color32, &'static str) {
    match (r10, gs_ok) {
        (R10Status::Ready, true) => (GREEN, tr("READY")),
        (R10Status::Searching, _) => (RED, tr("NO R10")),
        (_, false) => (AMBER, tr("NO GSPRO")),
        (R10Status::Connected, true) => (AMBER, tr("NOT READY")),
    }
}

/// Every word `status_word` can give (English source texts).
const STATUS_WORDS: [&str; 4] = ["READY", "NO R10", "NO GSPRO", "NOT READY"];

/// Overlay content: status, club and battery on top; last shot (or what's wrong) below.
/// Height of the two-line overlay with the impact graphics.
pub(super) const OVERLAY_IMPACT_H: f32 = 96.0;
/// Room in front of the first tile (the status colour bar) and after the last one.
const TILE_PAD_L: f32 = 18.0;
const TILE_PAD_R: f32 = 16.0;
/// Between two columns of tiles (a thin line in the middle).
const TILE_GAP: f32 = 24.0;
/// Room for a value's symbol in front of it.
const GLYPH_W: f32 = 22.0;
/// Size of the figures in a tile of double height.
const TALL_SIZE: f32 = 30.0;
/// Width of an empty place in the preview.
const EMPTY_TILE_W: f32 = 44.0;

/// What a tile shows.
#[derive(Clone, PartialEq, Debug)]
enum Cell {
    /// Dot and status word in the status colour.
    Status,
    /// A value, with its symbol in front; as wide as `wide`, the widest text it can take, so the
    /// overlay keeps its size from shot to shot.
    Value { glyph: Option<Glyph>, text: String, color: Color32, wide: String },
    /// Battery level of the R10 (`None`: not known, e.g. no R10 connected).
    Battery(Option<u8>),
    /// An impact graphic, filling its column.
    Graphic(OverlayItem),
    /// A value filling its column, in large figures.
    Tall(Box<Cell>),
}

/// What the overlay shows: the tiles (columns of two), or the status and what's wrong.
pub(super) struct OverlayContent {
    color: Color32,
    word: &'static str,
    /// The tiles in columns (top, bottom); the first one holds the status.
    columns: Vec<[Option<Cell>; 2]>,
    /// Not ready or not delivered: shown on a band over the other columns, whose last values stay
    /// dimmed underneath (the overlay keeps its size).
    message: Option<(String, Color32)>,
    one_line: bool,
    opacity: f32,
    /// Angles for the impact graphics (when a shot is shown).
    impact: Option<Impact>,
    /// Shown in the overlay page's preview: empty places stay visible to put something there.
    edit: bool,
}

/// What the impact graphics show, in degrees: angle of attack and club path need club data.
#[derive(Clone, Copy)]
struct Impact {
    spin_axis: f64,
    aoa: Option<f64>,
    vla: f64,
    path: Option<f64>,
    hla: f64,
}

impl OverlayContent {
    /// From the shared status.
    pub(super) fn live(style: &OverlayStyle, metric: bool, meters: bool) -> Self {
        let (r10, r10_detail, gs_ok, gs_detail, club, battery, last, to_pin) = state::with(|s| {
            let to_pin = s.gspro_result.and_then(|r| r.to_pin);
            let gspro = (s.gspro_connected, s.gspro_detail.clone());
            (s.r10, s.r10_detail.clone(), gspro.0, gspro.1, s.gspro_club.clone(), s.r10_battery, s.last_shot.clone(), to_pin)
        });
        let pin = to_pin.map(|m| pin_text(m, meters));
        let mut content = Self::new(style, metric, (r10, &r10_detail, gs_ok), club.as_deref(), battery, last.as_ref(), pin);
        // What is wrong with GSPro, e.g. that its Connect needs a reset.
        if r10 == R10Status::Ready && !gs_ok {
            content.message = Some((tr_detail(&gs_detail), AMBER));
        }
        content
    }

    fn new(
        style: &OverlayStyle,
        metric: bool,
        (r10, r10_detail, gs_ok): (R10Status, &str, bool),
        club: Option<&str>,
        battery: Option<u8>,
        last: Option<&state::LastShot>,
        pin: Option<String>,
    ) -> Self {
        let (color, word) = status_word(r10, gs_ok);
        let ready = r10 == R10Status::Ready && gs_ok;
        let message = match (last, ready) {
            (_, false) if r10 != R10Status::Ready => Some((tr_detail(r10_detail), AMBER)),
            (_, false) => Some((tr("GSPro not connected").to_string(), AMBER)),
            (Some(s), true) if !s.delivered => Some((tr("Last shot NOT delivered to GSPro").to_string(), AMBER)),
            _ => None,
        };
        // The last shot's values stay, also under a message; a dash where there is none yet.
        let shot = last;
        let (factor, unit) = if metric { (MPH_TO_KMH, "km/h") } else { (1.0, "mph") };
        let value = |glyph, text: Option<String>, wide: String| {
            Some(Cell::Value { glyph, text: text.unwrap_or_else(|| "–".into()), color: WHITE, wide })
        };
        let speed = |mph: f64| format!("{:.1} {unit}", mph * factor);
        let value_cell = |item: OverlayItem| -> Option<Cell> {
            let b = shot.map(|s| &s.ball);
            match item {
                OverlayItem::Status => Some(Cell::Status),
                OverlayItem::Club => Some(Cell::Value {
                    glyph: None,
                    text: club.map_or_else(|| "–".into(), club_name),
                    color: SOFT,
                    wide: club_name("PW"),
                }),
                OverlayItem::Battery => Some(Cell::Battery(battery)),
                OverlayItem::Pin => value(Some(Glyph::Pin), pin.clone().filter(|_| shot.is_some()), pin_text(888.0, true)),
                OverlayItem::ImpactSide | OverlayItem::ImpactTop => Some(Cell::Graphic(item)),
                OverlayItem::BallSpeed => value(Some(Glyph::BallSpeed), b.map(|b| speed(b.speed)), speed(288.8 / factor)),
                OverlayItem::ClubSpeed => {
                    value(Some(Glyph::ClubSpeed), shot.and_then(|s| s.club).map(|c| speed(c.speed)), speed(288.8 / factor))
                }
                OverlayItem::Smash => value(
                    Some(Glyph::Smash),
                    shot.and_then(|s| history::smash(&s.ball, s.club.as_ref())).map(|v| format!("{v:.2}")),
                    "8.88".into(),
                ),
                OverlayItem::Spin => value(Some(Glyph::Spin), b.map(|b| format!("{:.0}", b.total_spin)), "18888".into()),
                OverlayItem::Vla => value(None, b.map(|b| format!("VLA {}°", num(b.vla, 1))), "VLA -88.8°".into()),
                OverlayItem::Hla => value(None, b.map(|b| format!("HLA {}°", num(b.hla, 1))), "HLA -88.8°".into()),
                OverlayItem::SpinAxis => {
                    value(None, b.map(|b| format!("{} {}°", tr("Axis"), num(b.spin_axis, 1))), format!("{} -88.8°", tr("Axis")))
                }
                OverlayItem::Tempo => {
                    value(Some(Glyph::Tempo), shot.and_then(|s| s.tempo).map(|t| format!("{:.1} : 1", t.ratio())), "8.8 : 1".into())
                }
            }
        };
        // The value that looks misread stands out.
        let suspect = |item: OverlayItem| {
            shot.and_then(|s| s.misread).is_some_and(|m| match m {
                Misread::Spin => item == OverlayItem::Spin,
                Misread::Launch => item == OverlayItem::Vla,
                Misread::Direction => item == OverlayItem::Hla,
                Misread::SpinAxis => item == OverlayItem::SpinAxis,
                Misread::Smash => matches!(item, OverlayItem::Smash | OverlayItem::ClubSpeed),
            })
        };
        let cell = |item: OverlayItem| -> Option<Cell> {
            let mut cell = value_cell(item)?;
            if suspect(item)
                && let Cell::Value { color, .. } = &mut cell
            {
                *color = AMBER;
            }
            Some(cell)
        };
        // A graphic fills its column.
        let mut columns: Vec<[Option<Cell>; 2]> = style
            .layout
            .iter()
            .map(|[a, b]| match (a.filter(|i| i.is_graphic()), b.filter(|i| i.is_graphic())) {
                (Some(g), _) | (None, Some(g)) => [cell(g), None],
                // The same value in both places: one tile of double height.
                _ if a.is_some() && a == b && *a != Some(OverlayItem::Status) => [a.and_then(cell).map(|c| Cell::Tall(Box::new(c))), None],
                _ => [a.and_then(cell), b.and_then(cell)],
            })
            .collect();
        // The status always comes first.
        match columns.first_mut() {
            Some(first) if first[0] != Some(Cell::Status) => first[0] = Some(Cell::Status),
            None => columns.push([Some(Cell::Status), None]),
            _ => {}
        }
        let impact = last.map(|s| Impact {
            spin_axis: s.ball.spin_axis,
            aoa: s.club.map(|c| c.angle_of_attack),
            vla: s.ball.vla,
            path: s.club.map(|c| c.path),
            hla: s.ball.hla,
        });
        Self { color, word, columns, message, one_line: style.one_line, opacity: style.opacity, impact, edit: false }
    }

    /// The columns drawn with their place in the layout: all of them with a shot, else the first
    /// one and the battery. While editing (the preview), empty columns and tiles stay as places.
    fn shown_columns(&self) -> Vec<(usize, [Option<&Cell>; 2])> {
        self.columns
            .iter()
            .enumerate()
            .map(|(k, [a, b])| {
                (k, [a.as_ref(), b.as_ref()])
            })
            .filter(|(_, [a, b])| a.is_some() || b.is_some() || self.edit)
            .collect()
    }

    /// The tiles one after the other (one line): place in the layout and what it shows (`None`: an
    /// empty place while editing).
    fn shown_tiles(&self) -> Vec<(usize, usize, Option<&Cell>)> {
        let mut out = Vec::new();
        for (k, col) in self.shown_columns() {
            let whole = matches!(col[0], Some(Cell::Graphic(_) | Cell::Tall(_)));
            for (r, cell) in col.into_iter().enumerate() {
                if cell.is_some() || (self.edit && !(whole && r == 1)) {
                    out.push((k, r, cell));
                }
            }
        }
        out
    }

    /// Panel size at scale 1.
    pub(super) fn size(&self, ctx: &egui::Context) -> egui::Vec2 {
        let measure = |text: &str, font: FontId| ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, WHITE).size().x);
        let (tiles, height) = self.tiles(&measure);
        // A little room to spare: text laid out at another scale may come out a bit wider.
        let width = tiles.last().map_or(TILE_PAD_L, |t| t.0 + t.1) + TILE_PAD_R + 4.0;
        vec2(width.ceil(), height)
    }

    /// Where the columns go (left edge and width, from the panel's left) and the panel's height.
    fn tiles(&self, measure: &dyn Fn(&str, FontId) -> f32) -> (Vec<(f32, f32)>, f32) {
        let columns = self.shown_columns();
        let mut x = TILE_PAD_L;
        let mut out = Vec::new();
        let width = |cell: Option<&Cell>| cell.map_or(EMPTY_TILE_W, |c| cell_width(c, self.word, self.one_line, measure));
        if self.one_line {
            // One row: every tile after the other.
            for (_, _, cell) in self.shown_tiles() {
                let w = width(cell);
                out.push((x, w));
                x += w + 16.0;
            }
            return (out, 40.0);
        }
        for (_, col) in &columns {
            let whole = matches!(col[0], Some(Cell::Graphic(_) | Cell::Tall(_)));
            let w = if whole { width(col[0]) } else { col.iter().map(|c| width(*c)).fold(0.0, f32::max) };
            out.push((x, w));
            x += w + TILE_GAP;
        }
        // Taller with an impact graphic, which has its values underneath.
        let graphic = columns.iter().any(|(_, c)| matches!(c[0], Some(Cell::Graphic(_))));
        (out, if graphic { OVERLAY_IMPACT_H } else { OVERLAY_SIZE.y })
    }
}

/// Width of a tile's contents.
fn cell_width(cell: &Cell, word: &str, one_line: bool, measure: &dyn Fn(&str, FontId) -> f32) -> f32 {
    match cell {
        // As wide as the longest status word, so the overlay doesn't change with the status.
        Cell::Status => {
            let font = cond(if one_line { 20.0 } else { 24.0 });
            16.0 + STATUS_WORDS.iter().map(|w| measure(tr(w), font.clone())).fold(measure(word, font.clone()), f32::max)
        }
        Cell::Value { glyph, text, wide, .. } => {
            let font = FontId::proportional(14.0);
            (if glyph.is_some() { GLYPH_W } else { 0.0 }) + measure(text, font.clone()).max(measure(wide, font))
        }
        Cell::Battery(_) => 32.0 + measure("100 %", FontId::proportional(13.0)),
        // The panel spans the gaps on both sides.
        Cell::Graphic(_) => impact_panel_width(one_line) - if one_line { 16.0 } else { TILE_GAP },
        Cell::Tall(inner) if one_line => cell_width(inner, word, true, measure),
        Cell::Tall(inner) => match inner.as_ref() {
            Cell::Value { glyph, text, wide, .. } => {
                (if glyph.is_some() { GLYPH_W + 4.0 } else { 0.0 }) + measure(text, cond(TALL_SIZE)).max(measure(wide, cond(TALL_SIZE)))
            }
            other => cell_width(other, word, false, measure),
        },
    }
}

/// Put `item` into a tile; an impact graphic takes the whole column, and a value put where a
/// graphic was leaves the other place empty.
fn set_tile(layout: &mut [[Option<OverlayItem>; 2]], k: usize, row: usize, item: Option<OverlayItem>) {
    let Some(col) = layout.get_mut(k) else { return };
    let tall = col[0].is_some() && col[0] == col[1];
    if item.is_some_and(OverlayItem::is_graphic) || col.iter().flatten().any(|i| i.is_graphic()) {
        *col = [item, None];
    } else if tall {
        // A tile of double height keeps its height with the new value.
        *col = [item, item];
    } else {
        col[row] = item;
    }
}

/// Swap two tiles; when one of them fills its column, the two columns swap (never the first,
/// which holds the status).
fn swap_tiles(layout: &mut [[Option<OverlayItem>; 2]], (k1, r1): (usize, usize), (k2, r2): (usize, usize)) {
    if k1 >= layout.len() || k2 >= layout.len() {
        return;
    }
    // Graphics and tiles of double height fill their column: they move with it.
    let whole = |k: usize| layout[k].iter().flatten().any(|i| i.is_graphic()) || (layout[k][0].is_some() && layout[k][0] == layout[k][1]);
    if whole(k1) || whole(k2) {
        if k1 != 0 && k2 != 0 {
            layout.swap(k1, k2);
        }
        return;
    }
    let a = layout[k1][r1];
    layout[k1][r1] = layout[k2][r2];
    layout[k2][r2] = a;
}

/// Move a tile into a new column at `at` (dropped on the line between two columns). A tile
/// filling its column moves with it; a column left empty goes. Never before the status.
fn insert_tile(layout: &mut Vec<[Option<OverlayItem>; 2]>, (k, row): (usize, usize), at: usize) {
    if k == 0 && row == 0 || k >= layout.len() || at == 0 || at > layout.len() {
        return;
    }
    let col = layout[k];
    let whole = col.iter().flatten().any(|i| i.is_graphic()) || (col[0].is_some() && col[0] == col[1]);
    if whole {
        if k == 0 {
            return;
        }
        let moved = layout.remove(k);
        layout.insert(if at > k { at - 1 } else { at }, moved);
        return;
    }
    let Some(item) = col[row] else { return };
    layout[k][row] = None;
    layout.insert(at, [Some(item), None]);
    let old = if at <= k { k + 1 } else { k };
    if old != 0 && layout[old] == [None, None] {
        layout.remove(old);
    }
}

/// Name of a tile's contents on the overlay page.
fn overlay_item_name(item: OverlayItem) -> &'static str {
    match item {
        OverlayItem::Status => tr("Status"),
        OverlayItem::Club => tr("Club"),
        OverlayItem::BallSpeed => tr("Ball Speed"),
        OverlayItem::ClubSpeed => tr("Club Speed"),
        OverlayItem::Smash => tr("Smash Factor"),
        OverlayItem::Spin => tr("Total Spin"),
        OverlayItem::Vla => "VLA",
        OverlayItem::Hla => "HLA",
        OverlayItem::SpinAxis => tr("Spin Axis"),
        OverlayItem::Tempo => tr("Tempo"),
        OverlayItem::Pin => tr("Distance to the pin"),
        OverlayItem::Battery => tr("Battery"),
        OverlayItem::ImpactSide => tr("Impact from the side"),
        OverlayItem::ImpactTop => tr("Impact from above"),
    }
}

/// "139 m": the distance left to the pin, in metres or yards (shown after a flag).
fn pin_text(m: f64, meters: bool) -> String {
    if meters { format!("{m:.0} m") } else { format!("{:.0} yd", m * M_TO_YD) }
}

/// Draw the overlay into `rect`; returns where each tile of the layout was drawn (column, row,
/// rectangle), for picking what a tile shows in the preview.
pub(super) fn overlay_ui(p: &egui::Painter, rect: Rect, c: &OverlayContent) -> Vec<(usize, usize, Rect)> {
    p.rect(rect, 8.0, PAGE.gamma_multiply(c.opacity), Stroke::new(1.0, LINE), StrokeKind::Inside);
    p.rect_filled(
        Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(5.0, rect.height() - 2.0)),
        CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 },
        c.color,
    );

    let measure = |text: &str, font: FontId| p.layout_no_wrap(text.to_string(), font, WHITE).size().x;
    let (tiles, _) = c.tiles(&measure);
    let draw_cell = |cell: &Cell, at: egui::Pos2| match cell {
        Cell::Status => {
            p.circle_filled(pos2(at.x + 4.0, at.y), 5.0, c.color);
            caps_text(p, pos2(at.x + 16.0, at.y), egui::Align::Min, c.word, if c.one_line { 20.0 } else { 24.0 }, c.color);
        }
        Cell::Value { glyph, text, color, .. } => {
            let mut x = at.x;
            if let Some(g) = glyph {
                draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                x += GLYPH_W;
            }
            p.text(pos2(x, at.y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), *color);
        }
        Cell::Battery(level) => {
            let w = cell_width(cell, c.word, c.one_line, &measure);
            match level {
                Some(level) => draw_battery(p, pos2(at.x + w, at.y), *level),
                None => {
                    p.text(pos2(at.x + w, at.y), Align2::RIGHT_CENTER, "–", FontId::proportional(14.0), MUTED);
                }
            }
        }
        Cell::Tall(inner) => match inner.as_ref() {
            // In one line just the value; two lines draw it large (see below).
            Cell::Value { glyph, text, color, .. } if !c.one_line => {
                let mut x = at.x;
                if let Some(g) = glyph {
                    draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                    x += GLYPH_W + 4.0;
                }
                caps_text(p, pos2(x, at.y), egui::Align::Min, text, TALL_SIZE, *color);
            }
            Cell::Value { glyph, text, color, .. } => {
                let mut x = at.x;
                if let Some(g) = glyph {
                    draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                    x += GLYPH_W;
                }
                p.text(pos2(x, at.y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), *color);
            }
            Cell::Battery(Some(level)) => draw_battery(p, pos2(at.x + cell_width(inner, c.word, c.one_line, &measure), at.y), *level),
            _ => {}
        },
        Cell::Graphic(item) => {
            let Some(impact) = c.impact else { return };
            let gap = if c.one_line { 16.0 } else { TILE_GAP };
            let r = Rect::from_min_size(pos2(at.x - gap / 2.0, rect.top()), vec2(impact_panel_width(c.one_line), rect.height()));
            let draw: ImpactGraphic = if *item == OverlayItem::ImpactSide { impact_side } else { impact_top };
            draw(p, r, impact, !c.one_line);
        }
    };
    // An empty place while editing: a dashed outline.
    let placeholder = |r: Rect| {
        let r = r.shrink2(vec2(2.0, 4.0));
        let outline = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        p.extend(egui::Shape::dashed_line(&outline, Stroke::new(1.0, BUTTON_LINE), 4.0, 3.0));
    };
    let mut hit = Vec::new();
    if c.one_line {
        let y = rect.center().y;
        let mut tile = tiles.iter();
        for (k, r, cell) in c.shown_tiles() {
            let Some(&(x, w)) = tile.next() else { break };
            let area = Rect::from_min_size(pos2(rect.left() + x, rect.top()), vec2(w, rect.height()));
            match cell {
                Some(cell) => draw_cell(cell, pos2(rect.left() + x, y)),
                None => placeholder(area),
            }
            hit.push((k, r, area));
        }
        message_band(p, rect, &tiles, c);
        return hit;
    }
    // Two rows, a thin line between the columns.
    let inset = 20.0 + ((rect.height() - OVERLAY_SIZE.y) / 2.0).max(0.0);
    let rows = [rect.top() + inset, rect.bottom() - inset];
    let half = (rows[1] - rows[0]) / 2.0;
    let mut tile = tiles.iter();
    for (n, (k, col)) in c.shown_columns().into_iter().enumerate() {
        let Some(&(x, w)) = tile.next() else { break };
        if n > 0 {
            let lx = rect.left() + x - TILE_GAP / 2.0;
            p.line_segment([pos2(lx, rect.top() + 12.0), pos2(lx, rect.bottom() - 12.0)], Stroke::new(1.0, LINE));
        }
        if let Some(cell @ (Cell::Graphic(_) | Cell::Tall(_))) = col[0] {
            draw_cell(cell, pos2(rect.left() + x, rect.center().y));
            hit.push((k, 0, Rect::from_min_size(pos2(rect.left() + x - TILE_GAP / 2.0, rect.top()), vec2(w + TILE_GAP, rect.height()))));
        } else {
            for (r, cell) in col.iter().enumerate() {
                let y = rows[r];
                let area = Rect::from_center_size(pos2(rect.left() + x + w / 2.0, y), vec2(w + TILE_GAP - 2.0, half * 2.0));
                match cell {
                    Some(cell) => draw_cell(cell, pos2(rect.left() + x, y)),
                    None if c.edit => placeholder(Rect::from_center_size(area.center(), vec2(w + 8.0, half * 2.0 - 4.0))),
                    None => {}
                }
                hit.push((k, r, area));
            }
        }
    }
    message_band(p, rect, &tiles, c);
    hit
}

/// The message on a band over every column but the status, whose last values stay dimmed
/// underneath.
fn message_band(p: &egui::Painter, rect: Rect, tiles: &[(f32, f32)], c: &OverlayContent) {
    let (Some((text, color)), Some(&(x, _))) = (&c.message, tiles.get(1)) else { return };
    let gap = if c.one_line { 16.0 } else { TILE_GAP };
    let band = Rect::from_min_max(pos2(rect.left() + x - gap / 2.0 + 1.0, rect.top() + 1.0), rect.max - vec2(1.0, 1.0));
    p.rect(band, CornerRadius { nw: 0, sw: 0, ne: 7, se: 7 }, PAGE.gamma_multiply(0.94), Stroke::NONE, StrokeKind::Inside);
    p.line_segment([band.left_top(), band.left_bottom()], Stroke::new(2.0, *color));
    let inner = band.shrink2(vec2(14.0, 6.0));
    let mut job = egui::text::LayoutJob::single_section(
        format!("▲  {text}"),
        egui::TextFormat { font_id: FontId::proportional(14.0), color: *color, ..Default::default() },
    );
    job.wrap.max_width = inner.width();
    job.wrap.max_rows = if c.one_line { 1 } else { 3 };
    let galley = p.layout_job(job);
    let at = pos2(inner.left(), inner.center().y - galley.size().y / 2.0);
    // A solid ground right under the text, so no value shows through it.
    p.rect_filled(Rect::from_min_size(at, galley.size()).expand2(vec2(8.0, 3.0)), 4.0, PAGE);
    p.galley(at, galley, *color);
}

/// Symbols in front of the overlay's values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glyph {
    /// A golf ball with speed lines: ball speed.
    BallSpeed,
    /// A driver's head on its shaft: club speed.
    ClubSpeed,
    /// Club face meeting the ball: smash factor.
    Smash,
    /// A golf ball with an arrow around it: spin.
    Spin,
    /// A flag: distance to the pin.
    Pin,
    /// A stopwatch: swing tempo.
    Tempo,
}

/// A golf ball: white with a few dimples.
fn golf_ball(p: &egui::Painter, c: egui::Pos2, r: f32) {
    p.circle_filled(c, r, Color32::from_rgb(0xf2, 0xf5, 0xfa));
    let dimple = Color32::from_rgb(0xa6, 0xb2, 0xc6);
    for (dx, dy) in [(-0.35, -0.3), (0.25, -0.4), (0.4, 0.15), (-0.1, 0.1), (-0.4, 0.35), (0.15, 0.5)] {
        p.circle_filled(c + vec2(dx, dy) * r, r * 0.13, dimple);
    }
}

/// A 16 px symbol centred on `c`.
fn draw_glyph(p: &egui::Painter, c: egui::Pos2, glyph: Glyph, color: Color32) {
    match glyph {
        Glyph::BallSpeed => {
            golf_ball(p, c + vec2(3.0, 0.0), 5.2);
            for (dy, len) in [(-3.2, 3.5), (0.0, 5.5), (3.2, 3.5)] {
                p.line_segment([c + vec2(-3.5 - len, dy), c + vec2(-3.5, dy)], Stroke::new(1.3, color));
            }
        }
        Glyph::ClubSpeed => {
            // A driver: shaft down to a big round head, the face on the left.
            p.line_segment([c + vec2(6.5, -8.5), c + vec2(2.0, 1.5)], Stroke::new(1.4, color));
            let head: Vec<egui::Pos2> = [
                (2.8, 1.0),
                (1.0, 0.2),
                (-2.0, 0.0),
                (-5.0, 0.8),
                (-7.2, 2.6),
                (-7.8, 5.0),
                (-6.6, 7.0),
                (-3.0, 7.6),
                (1.5, 7.2),
                (3.6, 5.6),
                (3.8, 3.2),
            ]
            .iter()
            .map(|&(x, y)| c + vec2(x, y))
            .collect();
            p.add(egui::Shape::convex_polygon(head, color, Stroke::NONE));
            // The face, a little lighter.
            p.line_segment([c + vec2(-7.0, 2.8), c + vec2(-7.3, 5.6)], Stroke::new(1.2, Color32::from_rgb(0xe6, 0xec, 0xf6)));
        }
        Glyph::Smash => {
            // The face of a club meeting the ball, with a burst between them.
            p.add(egui::Shape::convex_polygon(
                vec![c + vec2(-8.0, -6.5), c + vec2(-5.0, -6.5), c + vec2(-5.0, 6.5), c + vec2(-8.0, 6.5)],
                color,
                Stroke::NONE,
            ));
            golf_ball(p, c + vec2(3.5, 0.0), 4.6);
            for a in [-0.7_f32, 0.0, 0.7] {
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + vec2(-3.6, 0.0) + d * 0.5, c + vec2(-3.6, 0.0) + d * 2.8], Stroke::new(1.1, AMBER));
            }
        }
        Glyph::Spin => {
            golf_ball(p, c, 4.4);
            // Most of a circle around the ball, ending in an arrow head.
            let r = 7.3;
            let (from, sweep) = (-2.6_f32, 4.4_f32);
            let pts: Vec<egui::Pos2> = (0..=20)
                .map(|k| {
                    let a = from + k as f32 / 20.0 * sweep;
                    c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            let end = pts[pts.len() - 1];
            p.add(egui::Shape::line(pts, Stroke::new(1.3, color)));
            let a = from + sweep;
            let (tangent, normal) = (vec2(-a.sin(), a.cos()), vec2(a.cos(), a.sin()));
            p.add(egui::Shape::convex_polygon(
                vec![end + tangent * 3.4, end - tangent * 1.0 + normal * 2.6, end - tangent * 1.0 - normal * 2.6],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Tempo => {
            // A stopwatch: round face, crown and button on top, a hand.
            let face = c + vec2(0.0, 1.2);
            p.circle_stroke(face, 6.0, Stroke::new(1.4, color));
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, -6.6), vec2(3.6, 1.8)), 0.6, color);
            p.line_segment([c + vec2(0.0, -5.7), c + vec2(0.0, -4.6)], Stroke::new(1.4, color));
            p.line_segment([c + vec2(4.6, -3.6), c + vec2(5.8, -4.8)], Stroke::new(1.4, color));
            p.line_segment([face, face + vec2(2.6, -2.8)], Stroke::new(1.4, AMBER));
            p.circle_filled(face, 1.1, color);
        }
        Glyph::Pin => {
            p.line_segment([c + vec2(-3.0, 7.5), c + vec2(-3.0, -7.5)], Stroke::new(1.4, color));
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-2.5, -7.5), c + vec2(6.5, -4.6), c + vec2(-2.5, -1.7)], RED, Stroke::NONE));
            p.add(egui::Shape::ellipse_filled(c + vec2(-3.0, 7.5), vec2(4.0, 1.3), color.gamma_multiply(0.6)));
        }
    }
}

/// Draws one impact graphic into a panel (with or without its caption).
type ImpactGraphic = fn(&egui::Painter, Rect, Impact, bool);

/// Width of one impact graphic: room for its caption with two lines, small in one line.
pub(super) fn impact_panel_width(one_line: bool) -> f32 {
    if one_line { 50.0 } else { 150.0 }
}

/// Room for a graphic's caption underneath (two lines only).
const IMPACT_CAPTION_H: f32 = 18.0;

/// Two values under a graphic, centred, each in its colour: "AoA 3.5 D · VLA 17.4°".
fn impact_caption(p: &egui::Painter, r: Rect, parts: &[(String, Color32)]) {
    let font = FontId::proportional(11.0);
    let sep = "  ·  ";
    let galleys: Vec<_> = parts.iter().map(|(t, c)| p.layout_no_wrap(t.clone(), font.clone(), *c)).collect();
    let sep_w = p.layout_no_wrap(sep.to_string(), font.clone(), MUTED).size().x;
    let total = galleys.iter().map(|g| g.size().x).sum::<f32>() + sep_w * (galleys.len().saturating_sub(1)) as f32;
    let y = r.bottom() - IMPACT_CAPTION_H / 2.0 - 2.0;
    let mut x = r.center().x - total / 2.0;
    for (k, g) in galleys.into_iter().enumerate() {
        if k > 0 {
            p.text(pos2(x, y), Align2::LEFT_CENTER, sep, font.clone(), MUTED);
            x += sep_w;
        }
        let w = g.size().x;
        p.galley(pos2(x, y - g.size().y / 2.0), g, WHITE);
        x += w;
    }
}

/// Map the part `view` of the mock's 118 × 94 drawing space into `r`, centred, same scale both ways.
fn impact_space(r: Rect, view: Rect) -> impl Fn(f32, f32) -> egui::Pos2 {
    let s = (r.width() / view.width()).min(r.height() / view.height());
    let origin = r.center() - view.center().to_vec2() * s;
    move |x, y| origin + vec2(x, y) * s
}

/// Filled arrow head at `tip`, pointing away from `from`.
fn arrow_head(p: &egui::Painter, from: egui::Pos2, tip: egui::Pos2, size: f32, color: Color32) {
    let d = (tip - from).normalized();
    let n = vec2(-d.y, d.x);
    let base = tip - d * size;
    p.add(egui::Shape::convex_polygon(vec![tip, base + n * size * 0.55, base - n * size * 0.55], color, Stroke::NONE));
}

/// Side view of the impact: the club head arriving at the angle of attack (blue, negative is
/// down), the ball leaving at the launch angle (white, dashed).
fn impact_side(p: &egui::Painter, r: Rect, i: Impact, labels: bool) {
    let draw = if labels { Rect::from_min_max(r.min + vec2(0.0, 2.0), r.max - vec2(0.0, IMPACT_CAPTION_H)) } else { r };
    // Club and ball stay between y 20 and 84: zoom into that band.
    let at = impact_space(draw, Rect::from_min_max(pos2(4.0, 20.0), pos2(114.0, 76.0)));
    let s = at(1.0, 0.0).x - at(0.0, 0.0).x;
    p.line_segment([at(6.0, 66.0), at(112.0, 66.0)], Stroke::new(2.0 * s, Color32::from_rgb(0x4b, 0x7a, 0x57)));
    if let Some(aoa) = i.aoa {
        let a = aoa.to_radians() as f32;
        let start = at(56.0 - 46.0 * a.cos(), 62.0 + 46.0 * a.sin());
        let end = at(56.0, 62.0);
        p.line_segment([start, end - (end - start).normalized() * 6.0 * s], Stroke::new(3.0 * s, ICON));
        arrow_head(p, start, end, 9.0 * s, ICON);
    }
    let v = i.vla.to_radians() as f32;
    let ball = at(60.0, 62.0);
    let out = at(60.0 + 50.0 * v.cos(), 62.0 - 50.0 * v.sin());
    p.extend(egui::Shape::dashed_line(&[ball, out - (out - ball).normalized() * 6.0 * s], Stroke::new(2.0 * s, WHITE), 4.0 * s, 3.0 * s));
    arrow_head(p, ball, out, 9.0 * s, WHITE);
    p.circle_filled(ball, 4.0 * s, WHITE);
    if labels {
        let mut parts: Vec<(String, Color32)> = i.aoa.map(|aoa| (format!("AoA {}", dir(aoa, "U", "D")), ICON)).into_iter().collect();
        parts.push((format!("VLA {}°", num(i.vla, 1)), WHITE));
        impact_caption(p, r, &parts);
    }
}

/// Top view of the impact over the target line (dashed): a broad arrow along the club path
/// (blue, no outline) with a narrow arrow for the start direction on top (white). The angles are
/// drawn three times larger; the numbers are the real ones.
fn impact_top(p: &egui::Painter, r: Rect, i: Impact, labels: bool) {
    let draw = if labels { Rect::from_min_max(r.min + vec2(0.0, 2.0), r.max - vec2(0.0, IMPACT_CAPTION_H)) } else { r };
    let at = impact_space(draw, Rect::from_min_max(pos2(0.0, 2.0), pos2(118.0, 92.0)));
    let s = at(1.0, 0.0).x - at(0.0, 0.0).x;
    p.extend(egui::Shape::dashed_line(&[at(59.0, 4.0), at(59.0, 90.0)], Stroke::new(1.0, Color32::from_rgb(0x4b, 0x6a, 0x8f)), 3.0 * s, 3.0 * s));
    // Outline of an arrow from the base (59, 88) at `deg`: shaft, neck, head, in drawing units.
    let arrow = |deg: f64, len: f32, shaft: f32, head_w: f32, head_l: f32| {
        let a = (deg * 3.0).to_radians() as f32;
        let (ux, uy) = (a.sin(), -a.cos());
        let (px, py) = (-uy, ux);
        let pt = |along: f32, across: f32| at(59.0 + ux * along + px * across, 88.0 + uy * along + py * across);
        let neck = len - head_l;
        let shaft_pts = vec![pt(0.0, -shaft / 2.0), pt(neck, -shaft / 2.0), pt(neck, shaft / 2.0), pt(0.0, shaft / 2.0)];
        let head_pts = vec![pt(neck, -head_w / 2.0), pt(len, 0.0), pt(neck, head_w / 2.0)];
        let outline = vec![
            pt(0.0, -shaft / 2.0),
            pt(neck, -shaft / 2.0),
            pt(neck, -head_w / 2.0),
            pt(len, 0.0),
            pt(neck, head_w / 2.0),
            pt(neck, shaft / 2.0),
            pt(0.0, shaft / 2.0),
        ];
        (shaft_pts, head_pts, outline)
    };
    if let Some(path) = i.path {
        let (shaft, head, _) = arrow(path, 78.0, 22.0, 40.0, 26.0);
        let blue = ICON.gamma_multiply(0.6);
        p.add(egui::Shape::convex_polygon(shaft, blue, Stroke::NONE));
        p.add(egui::Shape::convex_polygon(head, blue, Stroke::NONE));
    }
    // The start arrow, then the ball's flight curving by the spin axis (dashed, also exaggerated):
    // together they show the shot shape.
    let start_len = 46.0;
    let (shaft, head, outline) = arrow(i.hla, start_len, 6.0, 16.0, 14.0);
    let a = (i.hla * 3.0).to_radians() as f32;
    let heading = vec2(a.sin(), -a.cos());
    let tip = at(59.0 + heading.x * start_len, 88.0 + heading.y * start_len);
    // Curved only where the shape's name says so, like the announcement.
    let shape = Shape::of(i.hla, i.spin_axis, false);
    let bend = if shape.curve == Curve::None { 0.0 } else { (i.spin_axis as f32 * 2.2).clamp(-40.0, 40.0) };
    let control = at(59.0 + heading.x * (start_len + 22.0), 88.0 + heading.y * (start_len + 22.0));
    let end = at(59.0 + heading.x * 84.0 + bend, 88.0 + heading.y * 84.0);
    let curve: Vec<egui::Pos2> = (0..=16)
        .map(|k| {
            let t = k as f32 / 16.0;
            (tip.to_vec2() * (1.0 - t) * (1.0 - t) + control.to_vec2() * 2.0 * t * (1.0 - t) + end.to_vec2() * t * t).to_pos2()
        })
        .collect();
    p.extend(egui::Shape::dashed_line(&curve, Stroke::new(2.0 * s, WHITE), 4.0 * s, 3.0 * s));
    p.circle_filled(end, 2.5 * s, WHITE);
    p.add(egui::Shape::convex_polygon(shaft, WHITE, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(head, WHITE, Stroke::NONE));
    p.add(egui::Shape::closed_line(outline, Stroke::new(1.0, PAGE)));
    if labels {
        p.text(r.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, shape_name(shape), FontId::proportional(11.5), WHITE);
    }
    if labels {
        let mut parts: Vec<(String, Color32)> = i.path.map(|path| (format!("{} {}", tr("Path"), dir(path, "I-O", "O-I")), ICON)).into_iter().collect();
        parts.push((format!("Start {}", dir(i.hla, "R", "L")), WHITE));
        impact_caption(p, r, &parts);
    }
}

/// The overlay position as a small screen with eight spots; the middle turns the overlay off.
/// Sky and fairway behind the overlay previews, like GSPro.
fn backdrop(p: &egui::Painter, area: Rect) {
    const SKY: Color32 = Color32::from_rgb(0x1f, 0x3a, 0x5c);
    const FAIRWAY: Color32 = Color32::from_rgb(0x23, 0x50, 0x2f);
    p.rect_filled(area, 6.0, SKY);
    let ground = Rect::from_min_max(pos2(area.left(), area.top() + area.height() * 0.58), area.max);
    p.rect_filled(ground, CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, FAIRWAY);
    p.rect_stroke(area, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
}

/// Which overlay the page edits: tabs in the header of the left card (`card`), the status and
/// the `count` tiles, then *+*. Returns whether *+* was clicked.
fn overlay_tabs(ui: &mut egui::Ui, card: Rect, tab: &mut Option<usize>, count: usize) -> bool {
    let mut right = card.right() - 16.0;
    let mut add = false;
    // Right to left: +, the tiles from the last, the status.
    let mut tabs: Vec<(Option<Option<usize>>, String)> = vec![(None, "+".into())];
    for k in (0..count).rev() {
        let label = if count <= 2 { tr("TILE {n}").replace("{n}", &(k + 1).to_string()) } else { (k + 1).to_string() };
        tabs.push((Some(Some(k)), label));
    }
    tabs.push((Some(None), tr("STATUS").to_string()));
    for (which, label) in tabs {
        let w = ui.painter().layout_no_wrap(label.clone(), cond(15.0), WHITE).size().x;
        let r = Rect::from_min_max(pos2(right - w, card.top() + 6.0), pos2(right, card.top() + 38.0));
        let resp = ui.interact(r.expand2(vec2(4.0, 0.0)), ui.id().with(("overlay-tab", which)), Sense::click());
        let active = which == Some(*tab);
        let color = if active { WHITE } else if resp.hovered() { SOFT } else { MUTED };
        caps_text(ui.painter(), pos2(r.left(), card.top() + 21.0), egui::Align::Min, &label, 15.0, color);
        if active {
            ui.painter().line_segment([pos2(r.left(), card.top() + 37.0), pos2(r.right(), card.top() + 37.0)], Stroke::new(2.0, ACCENT));
        }
        let tip = if which.is_none() { tr("Add a tile overlay") } else { "" };
        let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            match which {
                Some(t) => *tab = t,
                None => add = true,
            }
        }
        right -= w + 16.0;
    }
    add
}

/// Where an overlay goes, next to its picker: the place, a hint, and *Move on screen* (or *Done*
/// while it is being moved).
fn place_text(ui: &mut egui::Ui, pos: OverlayPos, by_hand: bool, hint: &str, moving: &mut Option<Moving>, tile: Option<usize>) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let name = if by_hand && pos != OverlayPos::Off { tr("Placed by hand") } else { overlay_name(pos) };
    let p = ui.painter();
    p.text(r.left_center() - vec2(0.0, 18.0), Align2::LEFT_CENTER, name, FontId::proportional(14.0), WHITE);
    p.text(r.left_center() + vec2(0.0, 1.0), Align2::LEFT_CENTER, hint, FontId::proportional(12.0), MUTED);
    let here = moving.as_ref().is_some_and(|m| m.tile == tile);
    let (label, color) = if here { (tr("Done – drag it in GSPro"), ACCENT_HOVER) } else { (tr("Move on screen"), ICON) };
    let galley = p.layout_no_wrap(label.to_string(), FontId::proportional(12.5), color);
    let link = Rect::from_min_size(r.left_center() + vec2(0.0, 13.0), galley.size());
    let on = pos != OverlayPos::Off;
    let resp = ui.interact(link, ui.id().with(("move-overlay", tile)), if on { Sense::click() } else { Sense::hover() });
    let color = if !on { MUTED } else if resp.hovered() { WHITE } else { color };
    p.galley_with_override_text_color(link.min, galley, color);
    if !on {
        return;
    }
    let tip = if here { tr("Ends moving; the overlay stays where it is") } else { tr("Makes the overlay grabbable: drag it to its place in GSPro and let go") };
    if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *moving = if here { None } else { Some(Moving { tile, ..Moving::default() }) };
    }
}

/// Top left corner of an overlay of `size` (native points): placed by hand at `spot`, else at the
/// anchor, `OVERLAY_MARGIN` from the edges of the visible screen and never off its left or top.
fn overlay_origin(visible: Rect, size: egui::Vec2, (ax, ay): (f32, f32), spot: Option<[f32; 2]>) -> egui::Pos2 {
    if let Some([fx, fy]) = spot {
        let room = (visible.size() - size).max(egui::Vec2::ZERO);
        return visible.min + vec2(room.x * fx.clamp(0.0, 1.0), room.y * fy.clamp(0.0, 1.0));
    }
    let free = (visible.size() - size - vec2(2.0, 2.0) * OVERLAY_MARGIN).max(egui::Vec2::ZERO);
    visible.min + vec2(OVERLAY_MARGIN + free.x * ax, OVERLAY_MARGIN + free.y * ay)
}

/// Where an overlay dropped at `at` is kept: fractions of the room the visible screen leaves, so
/// it keeps its place on another screen size.
fn overlay_spot(visible: Rect, size: egui::Vec2, at: egui::Pos2) -> [f32; 2] {
    let room = visible.size() - size;
    let frac = |off: f32, room: f32| if room > 0.0 { (off / room).clamp(0.0, 1.0) } else { 0.0 };
    [frac(at.x - visible.left(), room.x), frac(at.y - visible.top(), room.y)]
}

/// An overlay being moved: grabbable all over, with a frame and a hint. `origin` is where it is
/// (native points), `zoom` the points of its pass in native points. Returns where it was dropped.
fn drag_overlay(ui: &egui::Ui, rect: Rect, moving: &mut Moving, origin: egui::Pos2, zoom: f32) -> Option<egui::Pos2> {
    let resp = ui.interact(rect, egui::Id::new("move-overlay"), Sense::drag());
    #[cfg(target_os = "macos")]
    let mouse = mac::mouse_location();
    #[cfg(not(target_os = "macos"))]
    let mouse: Option<egui::Pos2> = None;
    if resp.drag_started() {
        moving.grab = Some((mouse.unwrap_or(egui::Pos2::ZERO), origin));
        moving.dragged = egui::Vec2::ZERO;
    }
    if resp.dragged()
        && let Some((m0, o0)) = moving.grab
    {
        // The window moves under the mouse, so the system's mouse position is the steady one.
        moving.dragged += resp.drag_delta() * zoom;
        let by = mouse.map_or(moving.dragged, |m| m - m0);
        moving.at = Some(o0 + by);
    }
    ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("move-overlay")));
    fg.rect_stroke(rect, 8.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
    let galley = fg.layout_no_wrap(tr("Drag to its place").to_string(), FontId::proportional(12.0), WHITE);
    let badge = Rect::from_center_size(pos2(rect.center().x, rect.top() + 14.0), galley.size() + vec2(14.0, 6.0));
    fg.rect_filled(badge, 5.0, ACCENT);
    fg.galley(badge.min + vec2(7.0, 3.0), galley, WHITE);
    ui.ctx().request_repaint();
    if resp.drag_stopped() { moving.at.or(Some(origin)) } else { None }
}

/// The nine places; returns whether one was clicked. Placed `by_hand`, none is marked.
fn overlay_picker(ui: &mut egui::Ui, value: &mut OverlayPos, by_hand: bool) -> bool {
    let mut clicked = false;
    let (screen, _) = ui.allocate_exact_size(vec2(128.0, 72.0), Sense::hover());
    ui.painter().rect(screen, 6.0, PAGE, Stroke::new(2.0, FIELD_LINE), StrokeKind::Inside);
    let inner = screen.shrink2(vec2(10.0, 7.0));
    let cell = vec2(inner.width() / 3.0, inner.height() / 3.0);
    for (row, cells) in OverlayPos::GRID.iter().enumerate() {
        for (col, &pos) in cells.iter().enumerate() {
            let c = inner.min + vec2((col as f32 + 0.5) * cell.x, (row as f32 + 0.5) * cell.y);
            let hit = Rect::from_center_size(c, cell);
            let resp = ui.interact(hit, ui.id().with(("overlay_pos", row, col)), Sense::click());
            let selected = *value == pos && (!by_hand || pos == OverlayPos::Off);
            let p = ui.painter();
            if pos == OverlayPos::Off {
                let color = if selected { WHITE } else if resp.hovered() { SOFT } else { MUTED };
                p.text(c, Align2::CENTER_CENTER, tr("OFF"), FontId::proportional(10.0).clone(), color);
            } else {
                let spot = Rect::from_center_size(c, vec2(16.0, 10.0));
                if selected {
                    p.rect_filled(spot.expand(2.0), 3.0, ACCENT.gamma_multiply(0.35));
                }
                let fill = if selected { ACCENT } else if resp.hovered() { BUTTON_LINE } else { FIELD_LINE };
                p.rect_filled(spot, 2.0, fill);
            }
            if resp.on_hover_text(overlay_name(pos)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *value = pos;
                clicked = true;
            }
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{demo_state, save};
    use eframe::egui::FontFamily;

    /// Renders the top-view impact graphic for a few shot shapes side by side:
    /// `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --lib impact_shapes -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn impact_shapes() {
        use egui_kittest::Harness;

        i18n::set(Language::English);
        // (HLA, spin axis, path): straight, push-draw, pull-fade, hook, slice.
        let shots = [(0.3, 1.0, 0.5), (4.0, -6.0, 5.0), (-3.5, 7.0, -4.0), (-1.0, -14.0, 3.0), (1.0, 16.0, -6.0)];
        let w = impact_panel_width(false);
        let mut harness = Harness::builder()
            .with_size(vec2(w * shots.len() as f32, OVERLAY_IMPACT_H))
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_ui(move |ui| {
                if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                    for (k, &(hla, spin_axis, path)) in shots.iter().enumerate() {
                        let r = Rect::from_min_size(pos2(k as f32 * w, 0.0), vec2(w, OVERLAY_IMPACT_H));
                        ui.painter().rect_filled(r, 0.0, PAGE);
                        let impact = Impact { spin_axis, aoa: None, vla: 15.0, path: Some(path), hla };
                        impact_top(ui.painter(), r, impact, true);
                    }
                } else {
                    setup_style(ui.ctx());
                }
            });
        for _ in 0..3 {
            harness.step();
        }
        save(&harness.render().unwrap(), "impact-shapes.png");
    }

    /// The Overlay page's tile tab with a few tiles (range session).
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn tile_overlay_states() {
        use egui_kittest::Harness;
        use crate::settings::{Tile, TileSize};

        let language = if std::env::var("OPENLAUNCH_SCREENSHOT_LANG").as_deref() == Ok("de") { Language::German } else { Language::English };
        i18n::set(language);
        let _state = demo_state();
        state::with(|s| s.session = engine::demo_range());
        let tile = |kind, config| Tile::new(kind, TileSize::W2H2).with(&config);
        for (name, cfg) in [
            ("tile-overlay-dispersion.png", TileOverlay { pos: OverlayPos::TopRight, ..TileOverlay::default() }),
            (
                "tile-overlay-flight.png",
                TileOverlay { pos: OverlayPos::BottomLeft, tile: tile("flight", serde_json::json!({ "camera": "above" })), club_only: true, ..TileOverlay::default() },
            ),
            ("tile-overlay-moving.png", TileOverlay { pos: OverlayPos::Left, spot: Some([0.0, 0.45]), ..TileOverlay::default() }),
            ("tile-overlay-status.png", TileOverlay::default()),
            ("tile-overlay-play.png", TileOverlay::default()),
            ("tile-overlay-table.png", TileOverlay { pos: OverlayPos::Right, tile: tile("table", serde_json::json!({ "averages": true })), width: 460.0, ..TileOverlay::default() }),
        ] {
            let mut settings = Settings { tile_overlays: vec![cfg, TileOverlay::default()], ..Settings::default() };
            settings.keep_arrangement();
            settings.arrangements.insert(0, Arrangement { name: "Course".into(), ..Arrangement::default() });
            settings.arrangement = 1;
            settings.arrangements[1].name = "Range".into();
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder().with_size(BASE_SIZE).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
                setup_style(&cc.egui_ctx);
                App::new(settings, tx)
            });
            harness.state_mut().page = if name.contains("play") { Page::Play } else { Page::Overlay };
            harness.state_mut().overlay_tab = (!name.contains("status")).then_some(0);
            if name.contains("moving") {
                harness.state_mut().moving = Some(Moving { tile: Some(0), ..Moving::default() });
            }
            harness.state_mut().show_overlay = false;
            harness.state_mut().remember_size = false;
            for _ in 0..8 {
                harness.step();
            }
            save(&harness.render().unwrap(), name);
        }
    }

    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn overlay_symbols() {
        use egui_kittest::Harness;
        let glyphs = [Glyph::BallSpeed, Glyph::ClubSpeed, Glyph::Smash, Glyph::Spin, Glyph::Tempo, Glyph::Pin];
        let mut harness = Harness::builder().with_size(vec2(32.0 * glyphs.len() as f32, 32.0)).with_pixels_per_point(6.0).wgpu().build_ui(move |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, PAGE);
            for (k, g) in glyphs.iter().enumerate() {
                draw_glyph(ui.painter(), pos2(16.0 + 32.0 * k as f32, 16.0), *g, LABEL);
            }
        });
        harness.step();
        save(&harness.render().unwrap(), "overlay-symbols.png");
    }

    /// The overlay in each state: ready, R10 away, GSPro away (with the last shot dimmed under
    /// the message), no shot yet, and in one line – all of the same size.
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn overlay_states() {
        use egui_kittest::Harness;
        let _state = demo_state();
        let (ball, club) = engine::test_shot();
        let shot = state::LastShot { ball, club: Some(club), tempo: None, at: Instant::now(), delivered: true, shot_number: 1, misread: None };
        let one_line = OverlayStyle { one_line: true, ..OverlayStyle::default() };
        let states = [
            ("ready", OverlayStyle::default(), (R10Status::Ready, "Ready", true), Some(&shot)),
            ("no-r10", OverlayStyle::default(), (R10Status::Searching, "Searching for R10 …", true), Some(&shot)),
            ("no-gspro", OverlayStyle::default(), (R10Status::Ready, "Ready", false), Some(&shot)),
            ("no-shot", OverlayStyle::default(), (R10Status::Ready, "Ready", true), None),
            ("one-line", one_line, (R10Status::Searching, "Searching for R10 …", true), Some(&shot)),
        ];
        for (name, style, status, last) in states {
            let shot = last.cloned();
            let drawn = std::rc::Rc::new(std::cell::Cell::new(Rect::ZERO));
            let size_out = drawn.clone();
            let mut harness = Harness::builder()
                .with_size(vec2(1100.0, OVERLAY_IMPACT_H + 20.0))
                .with_pixels_per_point(2.0)
                .wgpu()
                .build_ui(move |ui| {
                    if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                        let battery = (status.0 != R10Status::Searching).then_some(82);
                        let content = OverlayContent::new(&style, true, status, Some("I7"), battery, shot.as_ref(), None);
                        let rect = Rect::from_min_size(ui.max_rect().min, content.size(ui.ctx()));
                        size_out.set(rect);
                        overlay_ui(ui.painter(), rect, &content);
                    } else {
                        setup_style(ui.ctx());
                    }
                });
            for _ in 0..3 {
                harness.step();
            }
            let r = drawn.get();
            println!("{name}: {:.0}×{:.0}", r.width(), r.height());
            let (min, size) = (r.min * 2.0, r.size() * 2.0);
            let image = image::imageops::crop_imm(&harness.render().unwrap(), min.x as u32, min.y as u32, size.x as u32, size.y as u32).to_image();
            save(&image, &format!("overlay-{name}.png"));
        }
    }

    #[test]
    fn overlays_placed_by_hand_keep_their_place() {
        let visible = Rect::from_min_size(pos2(0.0, 25.0), vec2(1512.0, 900.0));
        let size = vec2(364.0, 274.0);
        // Dropped somewhere: kept as fractions, back at the same spot.
        let at = pos2(40.0, 330.0);
        let spot = overlay_spot(visible, size, at);
        assert!((overlay_origin(visible, size, (0.0, 0.0), Some(spot)) - at).length() < 0.01);
        // Dropped half off the screen: kept on it.
        let spot = overlay_spot(visible, size, pos2(-50.0, 2000.0));
        assert_eq!(spot, [0.0, 1.0]);
        assert_eq!(overlay_origin(visible, size, (0.5, 0.5), Some(spot)), pos2(0.0, 25.0 + 900.0 - 274.0));
        // Without a spot, the anchor with the margin.
        assert_eq!(overlay_origin(visible, size, (0.0, 0.0), None), pos2(OVERLAY_MARGIN, 25.0 + OVERLAY_MARGIN));
    }

    #[test]
    fn overlay_graphics_move_as_columns() {
        use OverlayItem::*;
        let mut layout = OverlayItem::default_layout();
        // Ball speed onto the side graphic: the two columns swap.
        swap_tiles(&mut layout, (1, 0), (5, 0));
        assert_eq!(layout[1], [Some(ImpactSide), None]);
        assert_eq!(layout[5], [Some(BallSpeed), Some(ClubSpeed)]);
        // Never with the status column.
        swap_tiles(&mut layout, (0, 1), (1, 0));
        assert_eq!(layout[0], [Some(Status), Some(Club)]);
        // A graphic put into a tile takes the column; a value put there leaves the rest empty.
        set_tile(&mut layout, 3, 1, Some(ImpactTop));
        assert_eq!(layout[3], [Some(ImpactTop), None]);
        set_tile(&mut layout, 3, 0, Some(Tempo));
        assert_eq!(layout[3], [Some(Tempo), None]);
        set_tile(&mut layout, 3, 1, Some(Pin));
        assert_eq!(layout[3], [Some(Tempo), Some(Pin)]);
        // A tile of double height keeps its height with another value and moves as a column.
        layout[2] = [Some(BallSpeed), Some(BallSpeed)];
        set_tile(&mut layout, 2, 0, Some(Spin));
        assert_eq!(layout[2], [Some(Spin), Some(Spin)]);
        swap_tiles(&mut layout, (2, 0), (3, 1));
        assert_eq!((layout[2], layout[3]), ([Some(Tempo), Some(Pin)], [Some(Spin), Some(Spin)]));
    }

    #[test]
    fn overlay_tiles_go_between_columns() {
        use OverlayItem::*;
        let mut layout = OverlayItem::default_layout();
        let n = layout.len();
        // Spin onto the line between status and speeds: a column of its own there.
        insert_tile(&mut layout, (2, 1), 1);
        assert_eq!(layout.len(), n + 1);
        assert_eq!(layout[1], [Some(Spin), None]);
        assert_eq!(layout[3], [Some(Smash), None]);
        // Its old column emptied: gone. Smash after the last column.
        let end = layout.len();
        insert_tile(&mut layout, (3, 0), end);
        assert_eq!(layout.len(), n + 1);
        assert_eq!(layout.last(), Some(&[Some(Smash), None]));
        assert!(!layout.contains(&[None, None]));
        // Never before the status; the club below the status may leave.
        insert_tile(&mut layout, (1, 0), 0);
        assert_eq!(layout[1], [Some(Spin), None]);
        insert_tile(&mut layout, (0, 1), 2);
        assert_eq!(layout[0], [Some(Status), None]);
        assert_eq!(layout[2], [Some(Club), None]);
        // A graphic moves with its column.
        let side = layout.iter().position(|c| c[0] == Some(ImpactSide)).unwrap();
        insert_tile(&mut layout, (side, 0), 1);
        assert_eq!(layout[1], [Some(ImpactSide), None]);
    }

    /// Dragging a tile in the overlay preview onto another swaps the two.
    #[test]
    fn overlay_tiles_are_dragged_into_place() {
        use egui_kittest::Harness;

        let _state = demo_state();
        let settings = Settings { overlay: OverlayPos::TopRight, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(settings, tx)
        });
        harness.state_mut().page = Page::Overlay;
        harness.state_mut().show_overlay = false;
        harness.state_mut().remember_size = false;
        for _ in 0..3 {
            harness.step();
        }
        // Find two tiles by hovering over the preview: ball speed (column 1, top) and VLA (column 3, top).
        let ids: Vec<(usize, usize, Rect)> = harness.state().overlay_tile_rects.borrow().clone();
        let find = |k: usize, r: usize| ids.iter().find(|t| t.0 == k && t.1 == r).map(|t| t.2.center()).expect("tile");
        let (from, to) = (find(1, 0), find(3, 0));
        let press = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        for event in [egui::Event::PointerMoved(from), press(from, true), egui::Event::PointerMoved(from + vec2(15.0, 0.0)), egui::Event::PointerMoved(to), press(to, false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        let layout = &harness.state().saved.overlay_style.layout;
        assert_eq!((layout[1][0], layout[3][0]), (Some(OverlayItem::Vla), Some(OverlayItem::BallSpeed)));

        // Spin dropped on the line before the speeds' column: a new column there.
        let rects = harness.state().overlay_tile_rects.borrow().clone();
        let spin = rects.iter().find(|t| t.0 == 2 && t.1 == 1).unwrap().2.center();
        let line = rects.iter().filter(|t| t.0 == 1).map(|t| t.2.left()).fold(f32::MAX, f32::min);
        let to = pos2(line, spin.y);
        for event in [egui::Event::PointerMoved(spin), press(spin, true), egui::Event::PointerMoved(spin - vec2(15.0, 0.0)), egui::Event::PointerMoved(to), press(to, false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        assert_eq!(harness.state().saved.overlay_style.layout[1], [Some(OverlayItem::Spin), None]);

        // + after the last column adds an empty one, which can be clicked in the preview.
        let rects = harness.state().overlay_tile_rects.borrow().clone();
        let right = rects.iter().map(|t| t.2.right()).fold(f32::MIN, f32::max);
        let (top, bottom) = rects.iter().fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t.2.top()), b.max(t.2.bottom())));
        let plus = pos2(right, f32::midpoint(top, bottom));
        for event in [egui::Event::PointerMoved(plus), press(plus, true), press(plus, false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        let n = harness.state().saved.overlay_style.layout.len();
        assert_eq!(n, OverlayItem::default_layout().len() + 2);
        assert!(harness.state().overlay_tile_rects.borrow().iter().any(|t| t.0 == n - 1), "the empty column has places");
    }

    /// Scaling the overlay must not shrink the main window.
    #[test]
    fn overlay_scaling_leaves_the_main_window_alone() {
        use egui_kittest::Harness;

        let _state = crate::test_util::state_lock();
        let settings = Settings { overlay: OverlayPos::TopRight, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder()
            .with_size(vec2(1700.0, 780.0))
            .build_eframe(|cc| {
                setup_style(&cc.egui_ctx);
                App::new(settings, tx)
            });
        harness.state_mut().remember_size = false;
        for _ in 0..4 {
            // The overlay is placed on the monitor; kittest has none by default.
            harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().monitor_size =
                Some(vec2(2560.0, 1440.0));
            harness.step();
        }
        let ctx = &harness.ctx;
        assert!(ctx.zoom_factor() > 1.0);
        // The overlay is sized by the zoom of its own pass; the main window keeps its zoom.
        assert_eq!(ctx.layer_transform_to_global(egui::LayerId::background()), None);
        assert_eq!(ctx.layer_transform_to_global(overlay_layer()), None);
    }
}
