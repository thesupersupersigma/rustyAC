// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle, part two: the classes and HUD objects added after the
//! base.

use rustyac_gui::control::{Id, NO_CONTROL};
use rustyac_gui::font::FontType;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;
use rustyac_gui::widgets::WidgetEvent;
use rustyac_gui::widgets2::{Connected, Widget2};

use crate::gui_seq::Val;

#[derive(Default)]
pub struct Hud {
    /// the pop-overs drawn after the GUI in every frame
    pub pop_overs: Vec<Id>,
    pub hud: crate::gui_port_hud::HudPort,
}

impl Hud {
    pub fn render(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
        for pop_over in &self.pop_overs {
            gui.render_pop_over(*pop_over, dt, painter);
        }
    }
}

pub fn name(gui: &Gui, control: Id) -> String {
    if control == NO_CONTROL {
        String::from("<null>")
    } else {
        gui.controls[control].name.clone()
    }
}

pub fn class_name(gui: &Gui, id: Id) -> &'static str {
    match gui.widget2(id) {
        Widget2::ScrollBar(_) => "ScrollBar",
        Widget2::MovingBar(_) => "MovingBar",
        Widget2::TextBox(_) => "TextBox",
        Widget2::TextInput(_) => "TextInput",
        Widget2::PopOver(_) => "PopOver",
        Widget2::ConnectedLabel(_) => "ConnectedLabel",
        Widget2::TabBar(_) => "TabBar",
        Widget2::DefaultSystemMessage(_) => "DefaultSystemMessage",
        Widget2::More(_) => crate::gui_port3::class_name(gui, id),
    }
}

pub fn extra(gui: &Gui, id: Id) -> String {
    match gui.widget2(id) {
        Widget2::ScrollBar(s) => format!(" scroll={},{},{},{} dressed={}", s.min_value, s.max_value, s.step, s.value, s.dressed as u8),
        Widget2::MovingBar(m) => format!(" pressed={:08x},{:08x}", m.pressing_point[0].to_bits(), m.pressing_point[1].to_bits()),
        Widget2::TextBox(t) => format!(" lines={} \"{}\"", t.text_lines.len(), t.text_lines.join("|")),
        Widget2::TextInput(t) => format!(" focus={} max={}", t.has_focus as u8, t.max_number_of_char),
        Widget2::ConnectedLabel(c) => format!(" precision={}", c.precision),
        Widget2::TabBar(t) => format!(" tabs={:08x},{},{} mouse={},{}", t.tab_bar_x.to_bits(), t.top_index, t.roll_on_index, t.mouse[0], t.mouse[1]),
        Widget2::More(_) => crate::gui_port3::extra(gui, id),
        _ => String::new(),
    }
}

pub fn new_object(gui: &mut Gui, painter: &mut dyn Painter, class: &str, name: &str, flag: bool) -> Result<Id, String> {
    Ok(match class {
        "ScrollBar" => gui.new_scroll_bar(painter, name, flag),
        "TextBox" => gui.new_text_box(painter, name, if flag { FontType::Monospaced } else { FontType::Proportional }),
        "TextInput" => gui.new_text_input(painter, name),
        "PopOver" => gui.new_pop_over(painter, name),
        "ConnectedLabel" => gui.new_connected_label(painter, name, if flag { Connected::Float(0.0) } else { Connected::Int(0) }),
        "TabBar" => gui.new_tab_bar(painter, name),
        "DefaultSystemMessage" => gui.new_default_system_message(painter),
        other => return crate::gui_port3::new_object(gui, painter, other, name, flag),
    })
}

pub fn set(gui: &mut Gui, id: Id, name: &str, value: &Val) -> Result<(), String> {
    crate::gui_port3::set(gui, id, name, value)
}

pub fn call(gui: &mut Gui, painter: &mut dyn Painter, hud: &mut Hud, id: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    match what {
        "sbValue" => gui.scroll_bar_set_value(id, number as u32),
        "sbItems" => gui.scroll_bar_set_items_number(id, number as u32),
        "sbPerPage" => gui.scroll_bar_set_items_per_page(id, number as u32),
        "repeat" => gui.set_repeat_interval(id, number),
        "formatted" => gui.set_formatted_text(id, text),
        "popTitle" => gui.pop_over_set_label_title(id, text),
        "popText" => gui.pop_over_set_text(id, text),
        "drawPopOver" => {
            hud.pop_overs.retain(|p| *p != id);
            if number != 0.0 {
                hud.pop_overs.push(id);
            }
        }
        "connect" => {
            let label = gui.connected_label_mut(id);
            label.value = match label.value {
                Connected::Float(_) => Connected::Float(number),
                Connected::Int(_) => Connected::Int(number as i32),
            };
        }
        "suffix" => gui.connected_label_mut(id).suffix = text.to_string(),
        "labelText" => gui.connected_label_mut(id).label = text.to_string(),
        "precision" => gui.connected_label_mut(id).precision = number as i32,
        "addTab" => {
            gui.tab_bar_add_tab(painter, id, text);
        }
        "orderTabs" => {
            let count = gui.tab_bar(id).elements.len();
            gui.tab_bar_order_alphabetically(id, number as usize, count);
        }
        "message" => {
            let (title, description) = text.split_once('|').unwrap_or((text, ""));
            gui.default_system_message_set(id, title, description);
        }
        other => return crate::gui_port3::call(gui, painter, id, other, number, text),
    }
    Ok(())
}

pub fn event_text(gui: &Gui, event: &WidgetEvent) -> String {
    match event {
        WidgetEvent::CheckBoxChanged { check_box, value } => format!("check \"{}\" {}", name(gui, *check_box), *value as u8),
        WidgetEvent::SliderInteraction { slider, value } => format!("slider \"{}\" {:08x}", name(gui, *slider), value.to_bits()),
        WidgetEvent::CutExtremesChanged { slider, cut_in, cut_out } => format!("cut \"{}\" {:08x} {:08x}", name(gui, *slider), cut_in.to_bits(), cut_out.to_bits()),
        WidgetEvent::ScrollBarValueChanged { scroll_bar, value } => format!("scroll \"{}\" {}", name(gui, *scroll_bar), value),
        WidgetEvent::Text { control, text, kind: 0 } => format!("validate \"{}\" \"{}\"", name(gui, *control), text),
        WidgetEvent::Text { control, text, .. } => format!("key \"{}\" \"{}\"", name(gui, *control), text),
        WidgetEvent::TabSelected { tab_bar, old_index, new_index } => format!("tab \"{}\" {} {}", name(gui, *tab_bar), old_index, new_index),
        other => crate::gui_port3::event_text(gui, other),
    }
}
