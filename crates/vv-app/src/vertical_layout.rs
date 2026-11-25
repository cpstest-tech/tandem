//! «Verticale» layout for 9:16 work and small screens: the player at full
//! height, a compact timeline below, Library and Properties as drawers
//! beside the player. It only decides where the existing blocks go; the
//! desktop tile tree stays untouched for when the user switches back.

use serde::{Deserialize, Serialize};

use crate::theme;
use crate::workspace::{Block, BlockRects};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutMode {
    #[default]
    Desktop,
    Vertical,
}

pub(crate) const BAR_HEIGHT: f32 = 44.0;
const DRAWER_WIDTH: f32 = 360.0;
const MIN_PLAYER_WIDTH: f32 = 200.0;
const TIMELINE_SHARE: f32 = 0.34;
const TIMELINE_MIN: f32 = 150.0;
const TIMELINE_MAX: f32 = 340.0;
/// Below this width, or taller than wide, the window counts as narrow.
const NARROW_WIDTH: f32 = 900.0;
/// From this width, and wider than tall, the desktop layout fits again.
const WIDE_WIDTH: f32 = 1200.0;

#[derive(Debug, Default)]
pub(crate) struct State {
    pub library_open: bool,
    pub properties_open: bool,
    /// The layout proposed when the user answered «Non ora»: not proposed
    /// again until the window stops asking for it.
    dismissed: Option<LayoutMode>,
    /// Density applied to the egui style, `None` before the first frame.
    pub touch_applied: Option<bool>,
    /// Width of the area in the last vertical frame, for the drawer rules.
    pub last_width: f32,
}

impl State {
    pub fn toggle(&mut self, block: Block, width: f32) {
        let both_fit = fits_both(width);
        match block {
            Block::Library => {
                self.library_open = !self.library_open;
                if self.library_open && !both_fit {
                    self.properties_open = false;
                }
            }
            Block::Properties => {
                self.properties_open = !self.properties_open;
                if self.properties_open && !both_fit {
                    self.library_open = false;
                }
            }
            Block::Player | Block::Timeline => {}
        }
    }

    pub fn open(&mut self, block: Block, width: f32) {
        let open = match block {
            Block::Library => self.library_open,
            Block::Properties => self.properties_open,
            Block::Player | Block::Timeline => true,
        };
        if !open {
            self.toggle(block, width);
        }
    }

    /// The layout to propose for a window of `size` while `current` is on.
    pub fn proposal(&mut self, current: LayoutMode, size: egui::Vec2) -> Option<LayoutMode> {
        let wanted = suggested(current, size);
        if wanted.is_none() {
            self.dismissed = None;
        }
        wanted.filter(|w| self.dismissed != Some(*w))
    }

    pub fn dismiss(&mut self, proposed: LayoutMode) {
        self.dismissed = Some(proposed);
    }
}

/// The other layout when the window clearly suits it better.
pub(crate) fn suggested(current: LayoutMode, size: egui::Vec2) -> Option<LayoutMode> {
    let narrow = size.x < NARROW_WIDTH || size.y > size.x;
    let wide = size.x >= WIDE_WIDTH && size.x > size.y;
    match current {
        LayoutMode::Desktop if narrow => Some(LayoutMode::Vertical),
        LayoutMode::Vertical if wide => Some(LayoutMode::Desktop),
        _ => None,
    }
}

fn drawer_width(width: f32) -> f32 {
    DRAWER_WIDTH.min(width)
}

fn fits_both(width: f32) -> bool {
    width - 2.0 * (drawer_width(width) + theme::PANEL_GAP) >= MIN_PLAYER_WIDTH
}

/// The bar on top of `area` and where each block goes below it. A drawer
/// that would leave the player too narrow takes the whole top part.
pub(crate) fn rects(area: egui::Rect, library: bool, properties: bool) -> (egui::Rect, BlockRects) {
    let gap = theme::PANEL_GAP;
    let bar = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), BAR_HEIGHT));
    let body = area.with_min_y((bar.bottom() + gap).min(area.bottom()));
    let timeline_height = (body.height() * TIMELINE_SHARE)
        .clamp(TIMELINE_MIN, TIMELINE_MAX)
        .min(body.height() * 0.5);
    let timeline = body.with_min_y(body.bottom() - timeline_height);
    let top = body.with_max_y((timeline.top() - gap).max(body.top()));
    let mut rects = BlockRects::new();
    rects.insert(Block::Timeline, timeline);
    let (library, properties) = match (library, properties) {
        (true, true) if !fits_both(top.width()) => (false, true),
        other => other,
    };
    let drawer = drawer_width(top.width());
    let mut player = top;
    if library {
        let rect = top.with_max_x(top.left() + drawer);
        rects.insert(Block::Library, rect);
        player.min.x = rect.right() + gap;
    }
    if properties {
        let rect = top.with_min_x(top.right() - drawer);
        rects.insert(Block::Properties, rect);
        player.max.x = rect.left() - gap;
    }
    if player.width() >= MIN_PLAYER_WIDTH {
        rects.insert(Block::Player, player);
    } else if library != properties {
        // No room for the player: the drawer becomes a full sheet.
        let block = if library {
            Block::Library
        } else {
            Block::Properties
        };
        rects.insert(block, top);
    }
    (bar, rects)
}

impl crate::VenturiApp {
    pub(crate) fn vertical_layout(&self) -> bool {
        self.settings.shorts.layout == LayoutMode::Vertical
    }

    /// Whether `block` is on screen: laid out this frame in the vertical
    /// layout, docked or detached in the desktop one.
    pub(crate) fn block_shown(&self, block: Block) -> bool {
        if self.vertical_layout() {
            self.block_rects.contains(&block)
        } else {
            self.workspace.is_shown(block)
        }
    }

    /// Brings `block` on screen: opens its drawer, or reopens the closed tile.
    pub(crate) fn reveal_block(&mut self, block: Block) {
        if self.vertical_layout() {
            self.vertical.open(block, self.vertical.last_width);
        } else if !self.workspace.is_shown(block) {
            self.workspace.reopen(block);
        }
    }

    pub(crate) fn set_layout(&mut self, mode: LayoutMode) {
        if self.settings.shorts.layout != mode {
            self.settings.shorts.layout = mode;
            self.persist_settings();
        }
    }

    pub(crate) fn sync_layout_density(&mut self, ctx: &egui::Context) {
        let touch = self.vertical_layout();
        if self.vertical.touch_applied != Some(touch) {
            theme::set_touch_density(ctx, touch);
            self.vertical.touch_applied = Some(touch);
        }
    }

    /// A strip offering the layout that suits the window, when it differs.
    pub(crate) fn show_layout_proposal(&mut self, ui: &mut egui::Ui) {
        if !self.settings.shorts.propose_layout {
            return;
        }
        let size = ui.ctx().content_rect().size();
        let Some(proposed) = self.vertical.proposal(self.settings.shorts.layout, size) else {
            return;
        };
        let mut choice = None;
        egui::Panel::top("layout_proposal")
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_RAISED)
                    .inner_margin(egui::Margin::symmetric(theme::SPACE_2 as i8, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(match proposed {
                        LayoutMode::Vertical => t!("layout.propose_vertical"),
                        LayoutMode::Desktop => t!("layout.propose_desktop"),
                    });
                    let switch = crate::ui_kit::primary_button(match proposed {
                        LayoutMode::Vertical => t!("layout.switch_vertical"),
                        LayoutMode::Desktop => t!("layout.switch_desktop"),
                    });
                    if ui.add(switch).clicked() {
                        choice = Some(Some(proposed));
                    }
                    if ui.button(t!("layout.not_now")).clicked() {
                        choice = Some(None);
                    }
                    if ui.button(t!("layout.never")).clicked() {
                        self.settings.shorts.propose_layout = false;
                        self.persist_settings();
                    }
                });
            });
        match choice {
            Some(Some(mode)) => self.set_layout(mode),
            Some(None) => self.vertical.dismiss(proposed),
            None => {}
        }
    }

    /// Draws the drawer bar and returns where the blocks go.
    pub(crate) fn vertical_layout_ui(&mut self, ui: &mut egui::Ui) -> BlockRects {
        let area = ui.max_rect();
        self.vertical.last_width = area.width();
        let (bar, rects) = rects(
            area,
            self.vertical.library_open,
            self.vertical.properties_open,
        );
        let mut toggle = None;
        let mut desktop = false;
        let mut bar_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        bar_ui
            .painter()
            .rect_filled(bar, theme::panel_radius(), theme::BG_PANEL);
        bar_ui.add_space(theme::SPACE_1);
        if bar_ui
            .add(egui::Button::selectable(
                rects.contains_key(&Block::Library),
                t!("layout.library"),
            ))
            .on_hover_text(t!("layout.library_hint"))
            .clicked()
        {
            toggle = Some(Block::Library);
        }
        bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::SPACE_1);
            if ui
                .button(t!("layout.switch_desktop"))
                .on_hover_text(t!("layout.desktop_hint"))
                .clicked()
            {
                desktop = true;
            }
            if ui
                .add(egui::Button::selectable(
                    rects.contains_key(&Block::Properties),
                    t!("layout.properties"),
                ))
                .on_hover_text(t!("layout.properties_hint"))
                .clicked()
            {
                toggle = Some(Block::Properties);
            }
        });
        if let Some(block) = toggle {
            self.vertical.toggle(block, area.width());
            ui.ctx().request_repaint();
        }
        if desktop {
            self.set_layout(LayoutMode::Desktop);
            ui.ctx().request_repaint();
        }
        rects
    }
}

#[cfg(test)]
#[path = "tests/vertical_layout.rs"]
mod tests;
