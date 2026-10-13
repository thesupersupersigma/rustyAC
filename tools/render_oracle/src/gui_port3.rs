// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle, part three: the list box, the graph and the spinners.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;
use rustyac_gui::widgets::WidgetEvent;
use rustyac_gui::widgets3::{GraphReferenceAxis, ListBoxRowData, Widget3};

use crate::gui_seq::Val;

pub fn class_name(gui: &Gui, id: Id) -> &'static str {
    match gui.widget3(id) {
        Widget3::ListBoxRow(_) => "ListBoxRow",
        Widget3::ListBox(_) => "ListBox",
        Widget3::Graph(_) => "Graph",
        Widget3::Spinner(_) => "Spinner",
        Widget3::CustomSpinner(_) => "CustomSpinner",
    }
}

pub fn extra(gui: &Gui, id: Id) -> String {
    match gui.widget3(id) {
        Widget3::ListBox(l) => format!(" list={},{},{}", l.selected_item_index, l.pivot, l.items.len()),
        Widget3::ListBoxRow(r) => format!(" row={} cells={} {}", r.id, r.draw_row_background as u8, crate::gui_seq::bits4(&r.row_background)),
        Widget3::Graph(g) => {
            let sizes: Vec<String> = g.series.iter().map(|s| s.values.len().to_string()).collect();
            format!(" graph={:08x},{:08x},{},{} series={}", g.min_y.to_bits(), g.max_y.to_bits(), g.max_values_count, g.auto_adjust_max_value as u8, sizes.join(","))
        }
        Widget3::Spinner(s) => format!(
            " spinner={},{},{},{} counters={:08x},{:08x} items={} flags={}{}{}{}{}",
            s.value,
            s.min_value,
            s.max_value,
            s.step,
            s.counter.to_bits(),
            s.pression_counter.to_bits(),
            s.items.len(),
            s.display_popover as u8,
            s.draw_underline as u8,
            s.display_central_value as u8,
            s.display_label as u8,
            s.is_read_only as u8
        ),
        Widget3::CustomSpinner(s) => format!(" cspin={},{:08x},{}", s.mode, s.bar_position_normalized.to_bits(), s.press_to_increment as u8),
    }
}

pub fn new_object(gui: &mut Gui, painter: &mut dyn Painter, class: &str, name: &str, flag: bool) -> Result<Id, String> {
    Ok(match class {
        "ListBox" => {
            let (rows, columns) = if flag { (4, 1) } else { (6, 3) };
            gui.new_list_box(painter, name, rows, columns)
        }
        "Graph" => gui.new_graph(painter, name),
        "Spinner" => gui.new_spinner(painter, name, flag, false),
        "SpinnerMirrored" => gui.new_spinner(painter, name, flag, true),
        "CustomSpinner" => gui.new_custom_spinner(painter, name, if flag { 2 } else { 1 }),
        other => return Err(format!("the port side knows no class {other:?}")),
    })
}

pub fn set(gui: &mut Gui, id: Id, name: &str, value: &Val) -> Result<(), String> {
    match (name, value) {
        ("graph.minY", Val::F(v)) => gui.graph_mut(id).min_y = *v,
        ("graph.maxY", Val::F(v)) => gui.graph_mut(id).max_y = *v,
        ("graph.maxValues", Val::I(v)) => gui.graph_mut(id).max_values_count = *v as u32,
        ("graph.auto", Val::B(v)) => gui.graph_mut(id).auto_adjust_max_value = *v,
        ("spinner.homogeneous", Val::B(v)) => gui.spinner_mut(id).homogeneous_pressing_delay = *v,
        ("spinner.mult", Val::F(v)) => gui.spinner_mut(id).display_value_mult = *v,
        ("spinner.min", Val::I(v)) => gui.spinner_mut(id).min_value = *v,
        ("spinner.max", Val::I(v)) => gui.spinner_mut(id).max_value = *v,
        ("spinner.step", Val::I(v)) => gui.spinner_mut(id).step = *v,
        ("spinner.popover", Val::B(v)) => gui.spinner_mut(id).display_popover = *v,
        ("spinner.underline", Val::B(v)) => gui.spinner_mut(id).draw_underline = *v,
        ("spinner.central", Val::B(v)) => gui.spinner_mut(id).display_central_value = *v,
        ("spinner.label", Val::B(v)) => gui.spinner_mut(id).display_label = *v,
        ("spinner.textSize", Val::F(v)) => gui.spinner_mut(id).text_size = *v,
        ("spinner.readOnly", Val::B(v)) => gui.spinner_mut(id).is_read_only = *v,
        ("cspin.press", Val::B(v)) => gui.custom_spinner_mut(id).press_to_increment = *v,
        ("cspin.bar", Val::F(v)) => gui.custom_spinner_mut(id).bar_position_normalized = *v,
        (other, _) => return Err(format!("the port side knows no member {other:?}")),
    }
    Ok(())
}

pub fn call(gui: &mut Gui, painter: &mut dyn Painter, id: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    match what {
        "addItem" => {
            let (name, columns, back_color, forecolors) = crate::gui_seq::list_item(text, number);
            gui.list_box_add_item(id, &ListBoxRowData { name, category: number as u32, back_color, forecolors, columns });
        }
        "lbClear" => gui.list_box_clear(id),
        "lbSelect" => gui.list_box_select_item(id, number as i32),
        "lbSelectName" => gui.list_box_select_item_by_name(id, text),
        "lbTop" => gui.list_box_scroll_to_top(id),
        "lbCells" => gui.list_box_draw_cells_backgrounds(id, number != 0.0),
        "lbFont" => gui.list_box_set_items_font_size(id, number),
        "addSerie" => gui.graph_add_serie(id, crate::gui_seq::code_color(number)),
        "fill" => {
            let (serie, values) = crate::gui_seq::fill_values(text, number);
            for value in values {
                gui.graph_add_value(id, serie, value);
            }
        }
        "axis" => gui.graph_mut(id).axes.push(GraphReferenceAxis { color: [0.0, 1.0, 0.0], ref_value: number, is_vertical: text == "v" }),
        "clearSerie" => gui.graph_clear_serie(id, number as u32),
        "spValue" => gui.spinner_set_value(id, number as i32),
        "spValueItem" => gui.spinner_set_value_by_item(id, text),
        "spItem" => gui.spinner_add_item(id, text),
        "spLabel" => gui.spinner_set_label_text(id, text),
        "spUnits" => gui.spinner_mut(id).units = text.to_string(),
        "spArrows" => gui.spinner_set_draw_arrows(painter, id, number != 0.0),
        "spPos" => gui.spinner_set_position(id, number, text.parse().unwrap_or(0.0)),
        other => return Err(format!("the port side knows no call {other:?}")),
    }
    Ok(())
}

pub fn event_text(gui: &Gui, event: &WidgetEvent) -> String {
    match event {
        WidgetEvent::ListBoxRow { list_box, row, kind } => format!("list \"{}\" {} {}", crate::gui_port2::name(gui, *list_box), row, kind),
        WidgetEvent::SpinnerValueChanged { spinner, value } => format!("spin \"{}\" {}", crate::gui_port2::name(gui, *spinner), value),
        other => format!("{other:?}"),
    }
}
