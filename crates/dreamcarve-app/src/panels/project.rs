// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Project panel (`docs/specs/phase-4/functional.md`, capabilities 60,
//! 61 and 62; mock-up `03-stock-origin.svg`): what the Properties area
//! shows when nothing is selected, and what Project ▸ Stock and Origin…
//! opens. Five sections: Stock, Origin, Heights, Tolerances (folded away)
//! and Post-processor. Every row edits the project in place and applies
//! when it is left, one undo step, with no OK or Cancel (`experience.md`,
//! section 1); every field shows the file's validation message under
//! itself. What each row writes, its text and its message come from
//! `dc_app::project`; this module only draws them and hands the commands
//! to the window.
//!
//! The park point is typed, or picked: *Click on the view* arms the
//! Viewport, whose next click sets it ([`park_pick_armed`]). The armed
//! state is the window's view state, kept in egui's memory so the
//! Viewport reads it without the panel holding project data.

use dc_app::project::{self, Choice, Field, FieldStatus, Offer, OriginPreset};
use dc_app::{Command, StrKey, Strings};
use dc_job::{Finding, Tier};
use dc_project::{OriginZ, Project, SpindleControl, ToolChangeMode};
use eframe::egui::{self, Id, Ui};

use crate::shell::theme;
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::row::{self, GAP_PX, LABEL_PX};
use crate::widgets::section::section;
use crate::widgets::segmented::segmented;
use crate::widgets::toggle::toggle;
use crate::widgets::unit_field::{UnitField, field as unit_field};

/// The panel's content margin, px (`experience.md`, section 3).
const MARGIN_PX: i8 = 12;

/// The sections, by their `gui.toml` name, and whether each starts open.
const SECTIONS: [(&str, bool); 5] = [
    ("project.stock", true),
    ("project.origin", true),
    ("project.heights", true),
    ("project.tolerances", false),
    ("project.post", true),
];

/// Where the Viewport's park pick is armed.
fn park_pick_id() -> Id {
    Id::unique("dc.project.park_pick")
}

/// Whether the next click on the Viewport sets the park point.
pub(crate) fn park_pick_armed(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(park_pick_id()))
        .unwrap_or(false)
}

/// Arms or disarms the Viewport's park pick.
pub(crate) fn arm_park_pick(ctx: &egui::Context, armed: bool) {
    ctx.data_mut(|d| d.insert_temp(park_pick_id(), armed));
}

/// What every row reads.
struct Cx<'a> {
    strings: &'a Strings,
    project: &'a Project,
    findings: &'a [Finding],
    sections: &'a std::collections::BTreeMap<String, bool>,
}

impl Cx<'_> {
    fn open(&self, name: &str) -> bool {
        let default = SECTIONS
            .iter()
            .find(|(n, _)| *n == name)
            .is_some_and(|(_, open)| *open);
        self.sections.get(name).copied().unwrap_or(default)
    }
}

/// Draws the panel into the Properties area.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let cx = Cx {
        strings: state.app.strings(),
        project: state.app.project(),
        findings: state.app.warnings(),
        sections: &state.prefs.sections,
    };
    egui::ScrollArea::vertical()
        .id_salt("project_panel")
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(MARGIN_PX, 4))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(GAP_PX, 4.0);
                    stock(ui, &cx, out);
                    origin(ui, &cx, out);
                    heights(ui, &cx, out);
                    tolerances(ui, &cx, out);
                    post(ui, state, &cx, out);
                });
        });
}

/// A section with its header's located count; the action that flips it
/// when its header was clicked.
fn group(
    ui: &mut Ui,
    cx: &Cx<'_>,
    name: &str,
    title: StrKey,
    rows: (&[Field], &[Choice]),
    body: impl FnOnce(&mut Ui),
) -> Option<Action> {
    let open = cx.open(name);
    let statuses: Vec<FieldStatus<'_>> = rows
        .0
        .iter()
        .filter_map(|f| f.status(cx.findings))
        .chain(rows.1.iter().filter_map(|c| c.status(cx.findings)))
        .collect();
    let worst = [Tier::Error, Tier::Warning, Tier::Hint]
        .into_iter()
        .find(|tier| statuses.iter().any(|s| s.tier == *tier));
    let located = worst.map(|tier| (tier, statuses.len()));
    section(
        ui,
        Id::unique(name),
        cx.strings.get(title),
        open,
        located,
        body,
    )
    .then(|| Action::SetSection {
        name: name.to_owned(),
        open: !open,
    })
}

/// A typed field's row: the label, the field, and under it the message
/// that the field refused the text or the file's message on it.
fn field_row(
    ui: &mut Ui,
    cx: &Cx<'_>,
    field: Field,
    enabled: bool,
    disabled_reason: Option<&str>,
    out: &mut Vec<Action>,
) {
    let id = Id::unique(("project.field", field));
    let refused_id = id.with("refused");
    let refused: Option<String> = ui.data(|d| d.get_temp(refused_id));
    let status = field.status(cx.findings);
    let tier = refused
        .as_ref()
        .map(|_| Tier::Error)
        .or(status.map(|s| s.tier));
    let label = cx.strings.get(field.label());
    let value = field.text(cx.project);
    ui.horizontal(|ui| {
        row::label(ui, label, cx.strings.get(field.help()), Some(field.path()));
        let typed = unit_field(
            ui,
            UnitField {
                id,
                name: label,
                value: &value,
                unit: field.suffix(cx.project.display_units),
                placeholder: "",
                enabled,
                disabled_reason,
                tier,
            },
        );
        if let Some(text) = typed {
            match field.set(cx.project, &text) {
                Ok(command) => {
                    ui.data_mut(|d| d.remove::<String>(refused_id));
                    out.extend(command.map(Action::run));
                }
                Err(e) => {
                    ui.data_mut(|d| d.insert_temp(refused_id, e.to_string()));
                }
            }
        }
    });
    match (refused, status) {
        (Some(reason), _) => row::message(ui, Tier::Error, &reason),
        (None, Some(status)) => row::message(ui, status.tier, status.message),
        (None, None) => {}
    }
}

/// A chosen row: the label, `add`'s control, and the file's message.
fn choice_row(ui: &mut Ui, cx: &Cx<'_>, choice: Choice, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        row::label(
            ui,
            cx.strings.get(choice.label()),
            cx.strings.get(choice.help()),
            Some(choice.path()),
        );
        add(ui);
    });
    if let Some(status) = choice.status(cx.findings) {
        row::message(ui, status.tier, status.message);
    }
}

/// A dropdown of `options` with `current` ticked: the option picked, if
/// another, and the dropdown's response for its hover text.
fn dropdown<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    name: &str,
    current: T,
    options: &[(T, String)],
) -> (Option<T>, egui::Response) {
    let selected = options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    let mut picked = None;
    let width = ui.available_width();
    let response = egui::ComboBox::from_id_salt(id)
        .truncate()
        .selected_text(selected)
        .width(width)
        .show_ui(ui, |ui| {
            for (value, label) in options {
                if ui.selectable_label(*value == current, label).clicked() {
                    picked = Some(*value);
                }
            }
        })
        .response
        .accessible_name(name);
    (picked.filter(|p| *p != current), response)
}

fn stock(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let fields = [Field::StockWidth, Field::StockHeight, Field::StockThickness];
    let toggled = group(
        ui,
        cx,
        "project.stock",
        StrKey::ProjectStockTitle,
        (&fields, &[]),
        |ui| {
            for field in fields {
                field_row(ui, cx, field, true, None, out);
            }
            fit_row(ui, cx, out);
        },
    );
    out.extend(toggled);
}

/// *Fit to shapes* with the margin typed beside it (capability 61). The
/// margin is not a project field; it starts at `init`'s each session.
fn fit_row(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let units = cx.project.display_units;
    let id = Id::unique("project.fit_margin");
    let refused_id = id.with("refused");
    let margin: String = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| project::fit_margin_text(units));
    let refused: Option<String> = ui.data(|d| d.get_temp(refused_id));
    let label = cx.strings.get(StrKey::ProjectStockMargin);
    let fit = cx.strings.get(StrKey::ProjectStockFit);
    ui.horizontal(|ui| {
        row::label(
            ui,
            label,
            cx.strings.get(StrKey::ProjectStockMarginHelp),
            None,
        );
        if let Some(text) = unit_field(
            ui,
            UnitField {
                id,
                name: label,
                value: &margin,
                unit: dc_app::units::suffix(units),
                placeholder: "",
                enabled: true,
                disabled_reason: None,
                tier: refused.as_ref().map(|_| Tier::Error),
            },
        ) {
            ui.data_mut(|d| {
                d.insert_temp(id, text);
                d.remove::<String>(refused_id);
            });
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(LABEL_PX);
        if button(ui, ButtonSpec::new(Kind::Secondary, fit)).clicked() {
            match project::fit_stock(cx.project, &margin) {
                Ok(command) => out.push(Action::run(command)),
                Err(e) => {
                    ui.data_mut(|d| d.insert_temp(refused_id, e.to_string()));
                }
            }
        }
    });
    if let Some(reason) = refused {
        row::message(ui, Tier::Error, &reason);
    }
}

fn origin(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let preset = OriginPreset::of(&cx.project.origin.xy);
    let custom = [Field::OriginX, Field::OriginY];
    let fields: &[Field] = if preset == OriginPreset::Custom {
        &custom
    } else {
        &[]
    };
    let toggled = group(
        ui,
        cx,
        "project.origin",
        StrKey::ProjectOriginTitle,
        (fields, &[Choice::OriginXy, Choice::OriginZ]),
        |ui| {
            choice_row(ui, cx, Choice::OriginXy, |ui| {
                let options: Vec<(OriginPreset, String)> = OriginPreset::ALL
                    .iter()
                    .map(|p| (*p, cx.strings.get(p.label()).to_owned()))
                    .collect();
                if let (Some(picked), _) = dropdown(
                    ui,
                    "project.origin.xy",
                    cx.strings.get(Choice::OriginXy.label()),
                    preset,
                    &options,
                ) {
                    out.extend(project::set_origin_xy(cx.project, picked).map(Action::run));
                }
            });
            for field in fields {
                field_row(ui, cx, *field, true, None, out);
            }
            choice_row(ui, cx, Choice::OriginZ, |ui| {
                let options = [
                    cx.strings.get(StrKey::ProjectOriginTop),
                    cx.strings.get(StrKey::ProjectOriginBottom),
                ];
                let selected = usize::from(cx.project.origin.z == OriginZ::StockBottom);
                let width = ui.available_width();
                let picked = segmented(
                    ui,
                    Id::unique("project.origin.z"),
                    &options,
                    selected,
                    width,
                );
                let z = match picked {
                    Some(0) => Some(OriginZ::StockTop),
                    Some(_) => Some(OriginZ::StockBottom),
                    None => None,
                };
                if let Some(z) = z {
                    out.extend(project::set_origin_z(cx.project, z).map(Action::run));
                }
            });
        },
    );
    out.extend(toggled);
}

fn heights(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let fields = [Field::Clearance, Field::ToolChangeZ, Field::Park];
    let toggled = group(
        ui,
        cx,
        "project.heights",
        StrKey::ProjectHeightsTitle,
        (&fields, &[]),
        |ui| {
            field_row(ui, cx, Field::Clearance, true, None, out);
            // Offered where the definition documents G53; a height carried
            // over from another post stays editable so it can be cleared.
            let offer = project::offers(cx.project).tool_change_z;
            let set = cx.project.heights.tool_change_machine_z_mm.is_some();
            let reason = match &offer {
                Offer::Refused { reason } if !set => Some(reason.as_str()),
                _ => None,
            };
            field_row(ui, cx, Field::ToolChangeZ, reason.is_none(), reason, out);
            field_row(ui, cx, Field::Park, true, None, out);
            park_pick(ui, cx);
        },
    );
    out.extend(toggled);
}

/// *Click on the view*, under the park point's field.
fn park_pick(ui: &mut Ui, cx: &Cx<'_>) {
    let armed = park_pick_armed(ui.ctx());
    let picking = cx.strings.get(StrKey::ProjectHeightsPicking);
    ui.horizontal(|ui| {
        ui.add_space(LABEL_PX);
        let kind = if armed {
            Kind::Primary
        } else {
            Kind::Secondary
        };
        let spec = ButtonSpec::new(kind, cx.strings.get(StrKey::ProjectHeightsPick))
            .icon(Icon::Crosshair, theme::ICON_SMALL_PX)
            .tooltip(picking);
        if button(ui, spec).clicked() {
            arm_park_pick(ui.ctx(), !armed);
        }
    });
    if armed {
        row::hint(ui, picking);
    }
}

fn tolerances(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let fields = [Field::FlattenTolerance, Field::ArcFitTolerance];
    let toggled = group(
        ui,
        cx,
        "project.tolerances",
        StrKey::ProjectTolerancesTitle,
        (&fields, &[]),
        |ui| {
            field_row(ui, cx, Field::FlattenTolerance, true, None, out);
            let on = cx.project.arc_fit_tolerance_mm.is_some();
            let name = cx.strings.get(Field::ArcFitTolerance.label());
            ui.horizontal(|ui| {
                ui.add_space(LABEL_PX);
                if toggle(ui, on, name).clicked() {
                    out.extend(project::set_arc_fit(cx.project, !on).map(Action::run));
                }
            });
            // The sub-field dims when arc fitting is off (section 7, "Toggle").
            field_row(ui, cx, Field::ArcFitTolerance, on, None, out);
        },
    );
    out.extend(toggled);
}

fn post(ui: &mut Ui, state: &FrameState<'_>, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let fields = [
        Field::FileExtension,
        Field::SpindleDwell,
        Field::DecimalsMm,
        Field::DecimalsInch,
    ];
    let choices = [
        Choice::Post,
        Choice::SpindleControl,
        Choice::ToolChangeMode,
        Choice::CannedCycles,
    ];
    let toggled = group(
        ui,
        cx,
        "project.post",
        StrKey::ProjectPostTitle,
        (&fields, &choices),
        |ui| {
            controller(ui, state, cx, out);
            field_row(ui, cx, Field::FileExtension, true, None, out);
            choice_row(ui, cx, Choice::SpindleControl, |ui| {
                let options = [
                    (
                        SpindleControl::Automatic,
                        cx.strings.get(StrKey::ProjectPostAutomatic).to_owned(),
                    ),
                    (
                        SpindleControl::Manual,
                        cx.strings.get(StrKey::ProjectPostManual).to_owned(),
                    ),
                ];
                let current = cx.project.post_processor.spindle_control;
                if let (Some(picked), _) = dropdown(
                    ui,
                    "project.post.spindle",
                    cx.strings.get(Choice::SpindleControl.label()),
                    current,
                    &options,
                ) {
                    out.extend(project::set_spindle_control(cx.project, picked).map(Action::run));
                }
            });
            field_row(ui, cx, Field::SpindleDwell, true, None, out);
            tool_change(ui, cx, out);
            field_row(ui, cx, Field::DecimalsMm, true, None, out);
            field_row(ui, cx, Field::DecimalsInch, true, None, out);
            canned_cycles(ui, cx, out);
        },
    );
    out.extend(toggled);
}

/// The Controller list: the six bundled definitions at their latest
/// version, a note when the project is on an older one, and what the last
/// switch reset.
fn controller(ui: &mut Ui, state: &FrameState<'_>, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let posts = project::posts().unwrap_or_default();
    let current = cx.project.post_processor.id.as_str();
    let options: Vec<(&str, String)> = posts
        .iter()
        .map(|p| {
            let label = if p.id == current {
                project::post_label(cx.strings, cx.project)
            } else {
                cx.strings.fmt(
                    StrKey::ProjectPostVersionLatest,
                    &[("name", &p.name), ("version", &p.version.to_string())],
                )
            };
            (p.id, label)
        })
        .collect();
    choice_row(ui, cx, Choice::Post, |ui| {
        let width = ui.available_width();
        let mut picked = None;
        egui::ComboBox::from_id_salt("project.post.id")
            .truncate()
            .selected_text(project::post_label(cx.strings, cx.project))
            .width(width)
            .show_ui(ui, |ui| {
                for (id, label) in &options {
                    if ui.selectable_label(*id == current, label).clicked() {
                        picked = Some(*id);
                    }
                }
            })
            .response
            .accessible_name(cx.strings.get(Choice::Post.label()));
        if let Some(id) = picked {
            out.extend(project::switch_post(cx.project, id).map(Action::run));
        }
    });
    if let Some(latest) = project::newer_version(cx.project) {
        let version = cx.project.post_processor.definition_version.to_string();
        row::message(
            ui,
            Tier::Hint,
            &cx.strings.fmt(
                StrKey::ProjectPostOlder,
                &[("version", &version), ("latest", &latest.to_string())],
            ),
        );
    }
    let resets = state.app.last_resets();
    if resets.is_empty() {
        row::hint(ui, cx.strings.get(StrKey::ProjectPostKeeps));
    }
    for message in project::reset_messages(cx.strings, cx.project, resets) {
        row::message(ui, Tier::Warning, &message);
    }
}

/// One program per tool, or one with `M6` where the definition has it.
fn tool_change(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    choice_row(ui, cx, Choice::ToolChangeMode, |ui| {
        let options: Vec<(ToolChangeMode, String)> = project::tool_change_modes(cx.project)
            .into_iter()
            .map(|mode| {
                let key = match mode {
                    ToolChangeMode::SeparatePrograms => StrKey::ProjectPostSeparate,
                    ToolChangeMode::M6 => StrKey::ProjectPostM6,
                };
                (mode, cx.strings.get(key).to_owned())
            })
            .collect();
        let current = cx.project.post_processor.tool_change_mode;
        let (picked, response) = dropdown(
            ui,
            "project.post.tool_change",
            cx.strings.get(Choice::ToolChangeMode.label()),
            current,
            &options,
        );
        if let Offer::Refused { reason } = project::offers(cx.project).m6 {
            response.on_hover_text(reason);
        }
        if let Some(mode) = picked {
            out.extend(project::set_tool_change_mode(cx.project, mode).map(Action::run));
        }
    });
}

/// The canned-cycles checkbox where the definition has them; elsewhere a
/// note in its place, unless the file has them on and needs to clear it.
fn canned_cycles(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let on = cx.project.post_processor.use_canned_cycles;
    let offer = project::offers(cx.project).canned_cycles;
    if let (Offer::Refused { .. }, false) = (&offer, on) {
        let name = cx.project.post_processor.definition().map_or_else(
            |_| cx.project.post_processor.id.clone(),
            |d| d.post_processor().name.clone(),
        );
        ui.horizontal(|ui| {
            row::label(
                ui,
                cx.strings.get(Choice::CannedCycles.label()),
                cx.strings.get(Choice::CannedCycles.help()),
                Some(Choice::CannedCycles.path()),
            );
        });
        row::hint(
            ui,
            &cx.strings
                .fmt(StrKey::ProjectPostNoCanned, &[("name", &name)]),
        );
        return;
    }
    choice_row(ui, cx, Choice::CannedCycles, |ui| {
        let mut checked = on;
        let name = cx.strings.get(Choice::CannedCycles.label());
        if ui
            .checkbox(&mut checked, "")
            .accessible_name(name)
            .changed()
        {
            out.extend(project::set_canned_cycles(cx.project, checked).map(Action::run));
        }
    });
}

/// The command a click on the Viewport at `at_mm` makes while the park
/// pick is armed, for the Viewport to hand to the window.
pub(crate) fn park_at(project: &Project, at_mm: dc_geometry::Point) -> Option<Command> {
    project::set_park(project, at_mm)
}
