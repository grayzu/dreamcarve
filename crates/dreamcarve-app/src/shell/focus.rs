// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Keyboard focus (`experience.md`, section 8, "Keyboard focus"; section
//! 11; section 12, "Focus ring"; slice 19, #548): Tab walks the window
//! top-left to bottom-right, and the 2-px ring shows for keyboard focus
//! only.
//!
//! egui moves the focus to the next focusable widget in the order the
//! frame creates them, and the frame cannot create the panels in the walk's
//! order: egui lays out the status bar before the side panels so it spans
//! the window, the Properties panel before the Design Tree above it, and
//! the Viewport after the CAM panel because it takes the space left. So the
//! shell lets egui walk inside each [`Region`] and corrects a step that
//! crosses into the wrong one: [`mark`] is called as each region starts
//! to draw, [`finish`] after the frame, and when Tab has landed in a
//! region out of order the next frame moves the focus to the first widget
//! of the region that comes next (or, for Shift Tab, the last widget of
//! the one before), through egui's own focus moves.
//!
//! egui 0.37 does not say whether the focus came from the keyboard, so the
//! ring follows a flag set by Tab and the arrows and cleared by any pointer
//! press (section 12's fallback): a click into a field focuses it with no
//! ring.

use eframe::egui::{self, Context, Id, Key, Rect, Stroke, StrokeKind, Ui};

use crate::shell::theme::Palette;

/// A part of the window Tab walks through, in the walk's order: top-left
/// to bottom-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    /// The menu bar.
    Menu,
    /// The toolbar.
    Toolbar,
    /// The Design Tree.
    Tree,
    /// The Properties panel.
    Properties,
    /// The Viewport, its cards included.
    Viewport,
    /// The Warnings drawer, over the Viewport's bottom.
    Drawer,
    /// The CAM panel: the operations or tools list, then the editor.
    Cam,
    /// The status bar.
    Status,
}

impl Region {
    /// Every region in Tab's order.
    pub const ORDER: [Self; 8] = [
        Self::Menu,
        Self::Toolbar,
        Self::Tree,
        Self::Properties,
        Self::Viewport,
        Self::Drawer,
        Self::Cam,
        Self::Status,
    ];
}

/// How many frames a correction may take before it is given up: one per
/// region it can find empty.
const MAX_TRIES: u8 = 8;
/// Pixels a region's rectangle is widened by when placing a widget, so a
/// panel's resize handle on its edge is the panel's.
const EDGE_PX: f32 = 6.0;

/// A Tab press waiting to land.
#[derive(Debug, Clone, Copy)]
struct TabPress {
    forward: bool,
    /// The widget and region that had the focus when it was pressed.
    from: Option<(Id, Option<Region>)>,
    /// Frames since, with the focus not yet moved.
    frames: u8,
}

/// A correction: the focus is to move to the first widget of `target`
/// (forward) or its last (backward).
#[derive(Debug, Clone, Copy)]
struct Jump {
    forward: bool,
    target: Region,
    tries: u8,
}

/// What the router keeps between frames, in egui's memory.
#[derive(Debug, Clone, Default)]
struct Router {
    /// The region being drawn.
    drawing: Option<Region>,
    /// The regions drawn last frame, with their rectangles, in Tab's order.
    regions: Vec<(Region, Rect)>,
    /// The focused widget at the end of the last frame and its region.
    last: Option<(Id, Option<Region>)>,
    tab: Option<TabPress>,
    jump: Option<Jump>,
    /// Whether the focus came from the keyboard.
    keyboard: bool,
    /// Whether this frame's Escape left a focused widget (other than the
    /// Viewport), so it is not also a request to clear the selection.
    escape_left_widget: bool,
}

/// Where the router lives in egui's memory.
fn id() -> Id {
    Id::unique("dc_focus_router")
}

fn load(ctx: &Context) -> Router {
    ctx.data(|d| d.get_temp::<Router>(id())).unwrap_or_default()
}

fn store(ctx: &Context, router: Router) {
    ctx.data_mut(|d| d.insert_temp(id(), router));
}

/// The region after (`forward`) or before `from` that was drawn last
/// frame, wrapping; `from` itself when it is the only one.
fn step(regions: &[(Region, Rect)], from: Region, forward: bool) -> Region {
    let order: Vec<Region> = Region::ORDER
        .into_iter()
        .filter(|r| *r == from || regions.iter().any(|(drawn, _)| drawn == r))
        .collect();
    let at = order.iter().position(|r| *r == from).unwrap_or(0);
    let n = order.len().max(1);
    let next = if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    };
    order.get(next).copied().unwrap_or(from)
}

/// The first (`forward`) or last region drawn last frame.
fn end(regions: &[(Region, Rect)], forward: bool) -> Option<Region> {
    let mut drawn = Region::ORDER
        .into_iter()
        .filter(|r| regions.iter().any(|(d, _)| d == r));
    if forward {
        drawn.next()
    } else {
        drawn.next_back()
    }
}

/// The region whose rectangle holds the centre of widget `id`'s.
fn region_of(ctx: &Context, id: Id, regions: &[(Region, Rect)]) -> Option<Region> {
    let rect = ctx.read_response(id)?.rect;
    let centre = rect.center();
    // The bars first, then the side panels, then the Viewport between them.
    for wanted in [
        Region::Menu,
        Region::Toolbar,
        Region::Status,
        Region::Properties,
        Region::Tree,
        Region::Cam,
        Region::Drawer,
        Region::Viewport,
    ] {
        if let Some((_, r)) = regions.iter().find(|(region, _)| *region == wanted) {
            let widened = match wanted {
                Region::Menu | Region::Toolbar | Region::Status | Region::Viewport => *r,
                _ => r.expand(EDGE_PX),
            };
            if widened.contains(centre) {
                return Some(wanted);
            }
        }
    }
    None
}

/// At the start of a frame, before anything reads keys: notes a Tab press
/// and whether the focus now comes from the keyboard. Inside an open
/// sheet egui's own walk is the right one: it cycles within the sheet,
/// whose layer is the only one taking the focus.
pub(crate) fn begin(ctx: &Context, sheet_open: bool) {
    let mut router = load(ctx);
    router.drawing = None;
    let (tab, shift_tab, escape, arrows, pressed) = ctx.input(|i| {
        let key = |k: Key, m: egui::Modifiers| {
            i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                    if *key == k && modifiers.matches_exact(m))
            })
        };
        (
            key(Key::Tab, egui::Modifiers::NONE),
            key(Key::Tab, egui::Modifiers::SHIFT),
            key(Key::Escape, egui::Modifiers::NONE),
            [
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
            ]
            .into_iter()
            .any(|k| key(k, egui::Modifiers::NONE)),
            i.pointer.any_pressed(),
        )
    });
    let on_widget = router
        .last
        .is_some_and(|(_, region)| region != Some(Region::Viewport));
    router.escape_left_widget = escape && on_widget;
    if pressed {
        router.keyboard = false;
        router.tab = None;
        router.jump = None;
    }
    if tab || shift_tab {
        router.keyboard = true;
        router.jump = None;
        router.tab = (!sheet_open).then_some(TabPress {
            forward: tab,
            from: router.last,
            frames: 0,
        });
    } else if arrows && on_widget {
        router.keyboard = true;
    }
    store(ctx, router);
}

/// Region `region` starts to draw in `ui`: carries out a correction that
/// begins at its first widget, or ends at the last widget of the region
/// drawn before it.
pub(crate) fn mark(ui: &Ui, region: Region) {
    let ctx = ui.ctx();
    let mut router = load(ctx);
    close(ui, &router);
    router.drawing = Some(region);
    if let Some(jump) = router.jump
        && jump.forward
        && jump.target == region
    {
        // The first widget that asks for focus from here on takes it.
        ctx.memory_mut(|m| {
            if let Some(focused) = m.focused() {
                m.surrender_focus(focused);
            }
            m.move_focus(egui::FocusDirection::Next);
        });
    }
    store(ctx, router);
}

/// The last region has drawn: carries out a backward correction ending at
/// its last widget.
pub(crate) fn mark_end(ui: &Ui) {
    let ctx = ui.ctx();
    let mut router = load(ctx);
    close(ui, &router);
    router.drawing = None;
    store(ctx, router);
}

/// The region being drawn ends: when a backward correction targets it, its
/// last widget takes the focus. egui gives Shift Tab's focus to the widget
/// that asked for focus before the focused one, so a stand-in asks for it
/// here, with nothing focused.
fn close(ui: &Ui, router: &Router) {
    if let (Some(jump), Some(ending)) = (router.jump, router.drawing)
        && !jump.forward
        && jump.target == ending
    {
        let layer = ui.layer_id();
        ui.ctx().memory_mut(|m| {
            if let Some(focused) = m.focused() {
                m.surrender_focus(focused);
            }
            m.move_focus(egui::FocusDirection::Previous);
            m.interested_in_focus(Id::unique("dc_focus_stand_in"), layer);
        });
    }
}

/// Whether this frame's Escape left a focused widget, so it asks for
/// nothing more (the field it left restores its value).
pub(crate) fn escape_left_widget(ctx: &Context) -> bool {
    load(ctx).escape_left_widget
}

/// Whether the arrows and plain keys are the Viewport's: nothing has the
/// focus, or the Viewport has.
pub(crate) fn keys_free(ctx: &Context) -> bool {
    let router = load(ctx);
    match ctx.memory(egui::Memory::focused) {
        None => true,
        Some(focused) => region_of(ctx, focused, &router.regions) == Some(Region::Viewport),
    }
}

/// What the focus was after a frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Shown {
    /// The region holding it.
    pub region: Option<Region>,
    /// Where the ring was drawn.
    pub ring: Option<Rect>,
}

/// After the frame: `regions` were drawn there. Corrects a Tab that landed
/// out of order, and draws the ring around the focused widget when the
/// focus came from the keyboard: 2 px of `focus.ring`, 3 px outside the
/// control so no control shrinks (section 7, "focus"; section 11).
pub(crate) fn finish(ui: &Ui, regions: Vec<(Region, Rect)>) -> Shown {
    let ctx = ui.ctx();
    let mut router = load(ctx);
    router.regions = regions;
    let focused = ctx.memory(egui::Memory::focused);
    let region = focused.and_then(|f| region_of(ctx, f, &router.regions));
    // While a menu or dropdown is open its rows are not a region's.
    let popup = egui::Popup::is_any_open(ctx);

    if let Some(jump) = router.jump.take() {
        if region != Some(jump.target) && jump.tries < MAX_TRIES && !popup {
            // The target had nothing to focus: on to the next region.
            router.jump = Some(Jump {
                target: step(&router.regions, jump.target, jump.forward),
                tries: jump.tries + 1,
                ..jump
            });
        }
    } else if let Some(mut tab) = router.tab.take() {
        let from_id = tab.from.map(|(id, _)| id);
        let from_region = tab.from.and_then(|(_, region)| region);
        match focused {
            Some(now) if Some(now) != from_id => {
                // Landed. Inside one region egui's order is the walk's;
                // across regions it is checked.
                if region.is_some() && region != from_region && !popup {
                    let expected = match from_region {
                        Some(from) => Some(step(&router.regions, from, tab.forward)),
                        None => end(&router.regions, tab.forward),
                    };
                    if let Some(expected) = expected
                        && region != Some(expected)
                    {
                        router.jump = Some(Jump {
                            forward: tab.forward,
                            target: expected,
                            tries: 0,
                        });
                    }
                }
            }
            _ => {
                tab.frames += 1;
                if tab.frames < 3 {
                    router.tab = Some(tab);
                }
            }
        }
    }
    if router.jump.is_some() || router.tab.is_some() {
        ctx.request_repaint();
    }

    let mut ring = None;
    if router.keyboard
        && router.jump.is_none()
        && router.tab.is_none()
        && let Some(focused) = focused
        && let Some(response) = ctx.read_response(focused)
    {
        let at = response.rect.expand(3.0);
        let clip = region
            .and_then(|r| router.regions.iter().find(|(d, _)| *d == r))
            .map_or(ctx.content_rect(), |(_, rect)| rect.expand(EDGE_PX));
        let palette = Palette::current(ui);
        ctx.layer_painter(response.layer_id)
            .with_clip_rect(clip)
            .rect_stroke(
                at,
                6.0,
                Stroke::new(2.0, palette.focus_ring),
                StrokeKind::Middle,
            );
        ring = Some(at);
    }
    router.last = focused.map(|f| (f, region));
    store(ctx, router);
    Shown { region, ring }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(regions: &[Region]) -> Vec<(Region, Rect)> {
        regions.iter().map(|r| (*r, Rect::NOTHING)).collect()
    }

    #[test]
    fn steps_skip_regions_not_drawn_and_wrap() {
        use Region::*;
        let regions = drawn(&[Menu, Toolbar, Status, Properties, Tree, Cam, Viewport]);
        assert_eq!(step(&regions, Viewport, true), Cam, "the drawer is closed");
        assert_eq!(step(&regions, Status, true), Menu);
        assert_eq!(step(&regions, Menu, false), Status);
        assert_eq!(step(&regions, Tree, true), Properties);
        assert_eq!(end(&regions, true), Some(Menu));
        assert_eq!(end(&regions, false), Some(Status));
    }
}
