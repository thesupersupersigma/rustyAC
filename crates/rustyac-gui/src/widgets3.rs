// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The widget set of ksGUI, part three: `ListBoxRow` (0x1a8 bytes), `ListBox` (0x218) and
//! `Graph` (0x1b8); the spinners are in `spinner`.
//!
//! `ListBoxRow::ListBoxRow` 0x140255dd0, `clear` 0x140256240, `render` 0x1402563c0,
//! `setFontAlignment` 0x140256510, `setFontSize` 0x140256560, `setPosition` 0x1402565b0,
//! `setRow` 0x140256680, `setSize` 0x1402567c0; `ListBox::ListBox` 0x140248340, its handlers
//! 0x140248d80 (a row clicked) and 0x140248f90 (the scroll bar moved), `addItem` 0x1402495f0,
//! `clear` 0x1402496f0, `drawCellsBackgrounds` 0x1402497b0, `getItemIdFromName` 0x140249850,
//! `getSelectedItem` 0x140249980, `onMouseWheelMovedEvent` 0x1402499b0, `render` 0x140249c30,
//! `scrollListBoxToItem` 0x14024a030, `scrollToCenterOnSelected` 0x14024a1c0, `scrollToTop`
//! 0x14024a250, `selectItem` 0x14024a290, `selectItemByName` 0x14024a3c0, `setItemsFontSize`
//! 0x14024a5a0, `setSize` 0x14024a610; `Graph::Graph` 0x140247170, `addReferenceAxis`
//! 0x140247610, `addSerie` 0x140247620, `addValue` 0x140247700, `clearSerie` 0x140247760,
//! `getValuesAtPercentage` 0x1402477d0, `render` 0x1402478e0, `renderAxes` 0x140247ba0.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, GL_LINES, GL_LINE_STRIP};
use crate::widgets::{Widget, WidgetEvent, WidgetHandler};
use crate::widgets2::Widget2;

/// `ksgui::ListBoxRowData`: one item of a list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListBoxRowData {
    pub name: String,
    pub category: u32,
    pub back_color: [f32; 4],
    /// a text colour a column
    pub forecolors: Vec<[f32; 4]>,
    pub columns: Vec<String>,
}

/// `ksgui::ListBoxRow`: one line of a list on the screen, a label a column.
pub struct ListBoxRow {
    pub row_background: [f32; 4],
    pub id: i32,
    pub draw_row_background: bool,
    pub columns: Vec<Id>,
}

/// `ksgui::ListBox`: a table that shows a window of its items, moved by a scroll bar.
pub struct ListBox {
    pub allow_deselection: bool,
    pub allow_selection_on_click: bool,
    pub draw_underline: bool,
    pub draw_scrollbar_placeholder: bool,
    pub selection_mode: i32,
    /// `evOnListBoxItemClicked` (kind 0) and `evOnListBoxItemDeselected` (kind 1)
    pub handlers: Vec<WidgetHandler>,
    pub title_row: Id,
    pub bottom_row: Id,
    pub items: Vec<ListBoxRowData>,
    pub rows: Vec<Id>,
    pub column_number: u32,
    /// `category`: the colours a caller may give its rows (white, red, green, blue)
    pub category: Vec<(u32, [f32; 4])>,
    pub selected_item_index: i32,
    /// one past the last item shown
    pub pivot: i32,
    pub scroll_bar: Id,
}

/// `ksgui::ValueSerie`.
pub struct ValueSerie {
    pub color: [f32; 3],
    pub values: Vec<f32>,
}

/// `ksgui::GraphReferenceAxis`: a line at a share of the width or the height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphReferenceAxis {
    pub color: [f32; 3],
    pub ref_value: f32,
    pub is_vertical: bool,
}

/// `ksgui::Graph`: the last values of its series as lines.
pub struct Graph {
    pub min_y: f32,
    pub max_y: f32,
    pub max_values_count: u32,
    pub auto_adjust_max_value: bool,
    pub series: Vec<ValueSerie>,
    pub axes: Vec<GraphReferenceAxis>,
}

pub enum Widget3 {
    ListBoxRow(ListBoxRow),
    ListBox(ListBox),
    Graph(Graph),
    Spinner(crate::spinner::Spinner),
    CustomSpinner(crate::spinner::CustomSpinner),
    PitStopFuel(PitStopFuel),
}

/// `PitStopFuel` (0x180 bytes, PitStop.obj): the old mouse pit menu's fuel gauge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PitStopFuel {
    /// `fuelPercentage` (+0x178), `requestedFuelToAddPercentage` (+0x17c)
    pub fuel_percentage: f32,
    pub requested: f32,
    /// the share of the gauge the last click was at (what `onMouseDown` compares with the tank)
    pub clicked: Option<f32>,
}

/// The vertices of `Graph::localGL`, the drawer every graph shares.
const GRAPH_GL_VERTICES: u32 = 0x200;

impl Gui {
    pub fn widget3(&self, id: Id) -> &Widget3 {
        match self.widget2(id) {
            Widget2::More(w) => w,
            _ => panic!("control {id} is not a widget of the third part"),
        }
    }

    pub fn widget3_mut(&mut self, id: Id) -> &mut Widget3 {
        match self.widget2_mut(id) {
            Widget2::More(w) => w,
            _ => panic!("control {id} is not a widget of the third part"),
        }
    }

    pub fn set_widget3(&mut self, id: Id, widget: Widget3) {
        self.controls[id].kind = Kind::Widget(Box::new(Widget::More(Widget2::More(widget))));
    }

    // ----- ListBoxRow -----

    /// `ListBoxRow::ListBoxRow(name, columns, gui)` 0x140255dd0: a label a column, with the
    /// GUI's own font (the object itself, so a size set on a row is set for everyone).
    pub fn new_list_box_row(&mut self, painter: &mut dyn Painter, name: &str, columns: u32) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        let mut labels = Vec::new();
        for i in 0..columns {
            let label = self.new_label(painter, &format!("{name}_ROW_{i}\n"));
            self.set_text(label, "");
            self.controls[label].font = Some(self.font);
            self.controls[label].draw_border = false;
            self.controls[label].border_color = self.controls[id].border_color;
            labels.push(label);
            self.add_control(id, label);
        }
        self.set_widget3(id, Widget3::ListBoxRow(ListBoxRow { row_background: [0.5; 4], id: -1, draw_row_background: true, columns: labels }));
        id
    }

    pub fn list_box_row(&self, id: Id) -> &ListBoxRow {
        match self.widget3(id) {
            Widget3::ListBoxRow(r) => r,
            _ => panic!("control {id} is not a list box row"),
        }
    }

    pub fn list_box_row_mut(&mut self, id: Id) -> &mut ListBoxRow {
        match self.widget3_mut(id) {
            Widget3::ListBoxRow(r) => r,
            _ => panic!("control {id} is not a list box row"),
        }
    }

    /// `ListBoxRow::setFontAlignment` 0x140256510.
    pub fn list_box_row_set_font_alignment(&mut self, id: Id, align: i32) {
        for column in self.list_box_row(id).columns.clone() {
            self.controls[column].font_align = align;
        }
    }

    /// `ListBoxRow::setFontSize` 0x140256560: the size of the columns' font object.
    pub fn list_box_row_set_font_size(&mut self, id: Id, size: f32) {
        for column in self.list_box_row(id).columns.clone() {
            let font = self.controls[column].font.unwrap_or(self.font);
            self.fonts[font].scale = size;
        }
    }

    /// `ListBoxRow::clear` 0x140256240.
    pub fn list_box_row_clear(&mut self, id: Id) {
        for column in self.list_box_row(id).columns.clone() {
            self.set_text(column, "");
        }
    }

    /// `ListBoxRow::setPosition` 0x1402565b0: the columns share the width.
    pub fn list_box_row_set_position(&mut self, id: Id, x: f32, y: f32) {
        self.set_position(id, x, y);
        let columns = self.list_box_row(id).columns.clone();
        let inv = 1.0 / columns.len() as f32;
        for (i, column) in columns.into_iter().enumerate() {
            let r = self.controls[id].rect;
            self.set_position(column, ((r.right - r.left) * inv) * i as f32, 0.0);
        }
    }

    /// `ListBoxRow::setRow` 0x140256680: the texts and text colours of an item.
    pub fn list_box_row_set_row(&mut self, id: Id, data: &ListBoxRowData) {
        let columns = self.list_box_row(id).columns.clone();
        for (i, column) in columns.into_iter().enumerate().take(data.columns.len()) {
            self.set_text(column, &data.columns[i]);
            if let Some(color) = data.forecolors.get(i) {
                self.controls[column].fore_color = *color;
            }
        }
    }

    /// `ListBoxRow::render` 0x1402563c0.
    fn list_box_row_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let (columns, draw, color) = {
            let row = self.list_box_row(id);
            (row.columns.clone(), row.draw_row_background, row.row_background)
        };
        for column in columns {
            let c = self.controls[id].rect;
            let font = self.controls[column].font.unwrap_or(self.font);
            let x = self.controls[column].rect.left;
            self.set_position(column, x, ((c.bottom - c.top) * 0.5) - (self.fonts[font].scale * f32::from_bits(0x3f2a_aaab)));
        }
        if draw {
            let gl = self.controls[id].gl;
            painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
            painter.gl_quad(gl, r.left, r.top + 2.0, r.right - r.left, (r.bottom - r.top) - 4.0, false);
        }
        self.control_render(id, dt, painter);
    }

    // ----- ListBox -----

    /// `ListBox::ListBox(name, rows, columns, gui)` 0x140248340.
    pub fn new_list_box(&mut self, painter: &mut dyn Painter, name: &str, row_count: u32, column_number: u32) -> Id {
        let id = self.new_control(painter, name);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 25.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        let title_row = self.new_list_box_row(painter, "Title", column_number);
        self.add_control(id, title_row);
        let bottom_row = self.new_list_box_row(painter, "Bottom", column_number);
        self.set_visible(bottom_row, false);
        self.add_control(id, bottom_row);
        let scroll_bar = self.new_scroll_bar(painter, "SCROLL_BAR", true);
        self.scroll_bar_set_items_per_page(scroll_bar, row_count);
        self.set_visible(scroll_bar, true);
        let r = self.controls[id].rect;
        self.set_position(scroll_bar, ((r.right - r.left) - 25.0) + 1.0, 0.0);
        self.set_size(scroll_bar, 24.0, r.bottom - r.top);
        self.add_control(id, scroll_bar);
        self.scroll_bar_mut(scroll_bar).handlers.push(Rc::new(move |gui: &mut Gui, event: &WidgetEvent| {
            if let WidgetEvent::ScrollBarValueChanged { value, .. } = event {
                gui.scroll_list_box_to_item(id, *value as i32);
            }
        }));
        self.set_widget3(
            id,
            Widget3::ListBox(ListBox {
                allow_deselection: true,
                allow_selection_on_click: true,
                draw_underline: false,
                draw_scrollbar_placeholder: true,
                selection_mode: 0,
                handlers: Vec::new(),
                title_row,
                bottom_row,
                items: Vec::new(),
                rows: Vec::new(),
                column_number,
                category: vec![(0, [1.0; 4]), (1, [1.0, 0.0, 0.0, 1.0]), (2, [0.0, 1.0, 0.0, 1.0]), (3, [0.0, 0.0, 1.0, 1.0])],
                selected_item_index: -1,
                pivot: 0,
                scroll_bar,
            }),
        );
        for i in 0..row_count {
            let row = self.new_list_box_row(painter, &format!("{name}_ROW_{i}"), column_number);
            self.list_box_row_mut(row).id = i as i32;
            // a click on a row chooses the item it shows
            self.controls[row].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
                let list = gui.list_box(id);
                if !list.allow_selection_on_click || ev.control == crate::control::NO_CONTROL {
                    return;
                }
                let Kind::Widget(widget) = &gui.controls[ev.control].kind else { return };
                let Widget::More(Widget2::More(Widget3::ListBoxRow(clicked))) = &**widget else { return };
                let i = list.pivot - list.rows.len() as i32 + clicked.id;
                if i < 0 || i as usize >= list.items.len() {
                    return;
                }
                gui.list_box_select_item(id, i);
            }));
            self.add_control(id, row);
            self.list_box_mut(id).rows.push(row);
        }
        let list = self.list_box_mut(id);
        list.pivot = list.rows.len() as i32;
        id
    }

    pub fn list_box(&self, id: Id) -> &ListBox {
        match self.widget3(id) {
            Widget3::ListBox(l) => l,
            _ => panic!("control {id} is not a list box"),
        }
    }

    pub fn list_box_mut(&mut self, id: Id) -> &mut ListBox {
        match self.widget3_mut(id) {
            Widget3::ListBox(l) => l,
            _ => panic!("control {id} is not a list box"),
        }
    }

    /// `ListBox::setSize` 0x14024a610: a row is the list's height by the rows of a page; the
    /// title row sits one row above the list, the bottom row under it.
    fn list_box_set_size(&mut self, id: Id, w: f32, h: f32) {
        self.control_set_size(id, w, h);
        let (scroll_bar, title_row, bottom_row, rows) = {
            let l = self.list_box(id);
            (l.scroll_bar, l.title_row, l.bottom_row, l.rows.clone())
        };
        let r = self.controls[id].rect;
        self.set_size(scroll_bar, 24.0, r.bottom - r.top);
        self.set_position(scroll_bar, ((r.right - r.left) - 25.0) + 1.0, 0.0);
        let per_page = self.scroll_bar(scroll_bar).min_value as f32;
        let bar_width = |gui: &Gui| gui.controls[scroll_bar].rect.right - gui.controls[scroll_bar].rect.left;
        let width = w - bar_width(self);
        self.set_size(title_row, width, h / per_page);
        let r = self.controls[id].rect;
        self.list_box_row_set_position(title_row, 0.0, -((r.bottom - r.top) / per_page));
        let width = w - bar_width(self);
        self.set_size(bottom_row, width, h / per_page);
        self.list_box_row_set_position(bottom_row, 0.0, r.bottom - r.top);
        for (i, row) in rows.into_iter().enumerate() {
            let width = w - bar_width(self);
            self.set_size(row, width, h / per_page);
            let r = self.controls[id].rect;
            self.list_box_row_set_position(row, 0.0, ((r.bottom - r.top) / per_page) * i as f32);
        }
    }

    /// `rows / 2 + selected`, kept between 0 and the number of items.
    fn list_box_centre_target(&self, id: Id) -> i32 {
        let l = self.list_box(id);
        let t = l.rows.len() as i32 / 2 + l.selected_item_index;
        if t <= l.items.len() as i32 {
            t.max(0)
        } else {
            l.items.len() as i32
        }
    }

    /// `ListBox::addItem` 0x1402495f0: with nothing chosen the list goes to its end.
    pub fn list_box_add_item(&mut self, id: Id, data: &ListBoxRowData) {
        let list = self.list_box_mut(id);
        list.items.push(data.clone());
        let (scroll_bar, count, selected) = (list.scroll_bar, list.items.len() as i32, list.selected_item_index);
        self.scroll_bar_set_items_number(scroll_bar, count as u32);
        let t = if selected < 0 { count } else { self.list_box_centre_target(id) };
        self.scroll_bar_set_value(scroll_bar, t as u32);
        self.scroll_list_box_to_item(id, t);
    }

    /// `ListBox::clear` 0x1402496f0.
    pub fn list_box_clear(&mut self, id: Id) {
        for row in self.list_box(id).rows.clone() {
            self.list_box_row_clear(row);
        }
        let list = self.list_box_mut(id);
        list.items.clear();
        list.selected_item_index = -1;
        let scroll_bar = list.scroll_bar;
        self.scroll_bar_set_items_number(scroll_bar, 0);
    }

    /// `ListBox::drawCellsBackgrounds` 0x1402497b0.
    pub fn list_box_draw_cells_backgrounds(&mut self, id: Id, draw: bool) {
        for row in self.list_box(id).rows.clone() {
            self.list_box_row_mut(row).draw_row_background = draw;
        }
    }

    /// `ListBox::getItemIdFromName` 0x140249850.
    pub fn list_box_item_id_from_name(&self, id: Id, name: &str) -> i32 {
        self.list_box(id).items.iter().position(|item| item.name == name).map(|i| i as i32).unwrap_or(-1)
    }

    /// `ListBox::getSelectedItem` 0x140249980.
    pub fn list_box_selected_item(&self, id: Id) -> Option<&ListBoxRowData> {
        let l = self.list_box(id);
        if l.selected_item_index >= 0 && !l.items.is_empty() {
            l.items.get(l.selected_item_index as usize)
        } else {
            None
        }
    }

    /// `ListBox::scrollListBoxToItem` 0x14024a030.
    pub fn scroll_list_box_to_item(&mut self, id: Id, t: i32) {
        let l = self.list_box(id);
        if t >= 0 && t <= l.items.len() as i32 {
            let first = (t - self.scroll_bar(l.scroll_bar).min_value as i32).max(0) as usize;
            let n = l.items.len().min(l.rows.len());
            let (rows, items) = (l.rows.clone(), l.items.clone());
            for k in 0..n {
                if let Some(item) = items.get(first + k) {
                    self.list_box_row_set_row(rows[k], item);
                }
            }
            let list = self.list_box_mut(id);
            list.pivot = if list.rows.len() as i32 <= t { t } else { list.rows.len() as i32 };
        }
    }

    /// `ListBox::scrollToCenterOnSelected` 0x14024a1c0.
    pub fn list_box_scroll_to_center_on_selected(&mut self, id: Id) {
        let t = self.list_box_centre_target(id);
        let scroll_bar = self.list_box(id).scroll_bar;
        self.scroll_bar_set_value(scroll_bar, t as u32);
        self.scroll_list_box_to_item(id, t);
    }

    /// `ListBox::scrollToTop` 0x14024a250.
    pub fn list_box_scroll_to_top(&mut self, id: Id) {
        let scroll_bar = self.list_box(id).scroll_bar;
        let v = self.scroll_bar(scroll_bar).min_value;
        self.scroll_bar_set_value(scroll_bar, v);
        self.scroll_list_box_to_item(id, v as i32);
    }

    /// `ListBox::selectItem` 0x14024a290: the chosen item again lets go of it (when that is
    /// allowed).
    pub fn list_box_select_item(&mut self, id: Id, i: i32) {
        if i < 0 {
            return;
        }
        let list = self.list_box_mut(id);
        let handlers = list.handlers.clone();
        if i == list.selected_item_index && list.allow_deselection {
            list.selected_item_index = -1;
            self.fire_widget(handlers, WidgetEvent::ListBoxRow { list_box: id, row: i, kind: 1 });
        } else {
            list.selected_item_index = i;
            self.fire_widget(handlers, WidgetEvent::ListBoxRow { list_box: id, row: i, kind: 0 });
            self.list_box_scroll_to_center_on_selected(id);
        }
    }

    /// `ListBox::selectItemByName` 0x14024a3c0: the last item with a column of that text.
    pub fn list_box_select_item_by_name(&mut self, id: Id, name: &str) {
        let found = self.list_box(id).items.iter().rposition(|item| item.columns.iter().any(|c| c == name));
        if let Some(i) = found {
            self.list_box_select_item(id, i as i32);
        }
    }

    /// `ListBox::setItemsFontSize` 0x14024a5a0.
    pub fn list_box_set_items_font_size(&mut self, id: Id, size: f32) {
        for row in self.list_box(id).rows.clone() {
            self.list_box_row_set_font_size(row, size);
        }
    }

    /// `ListBox::render` 0x140249c30.
    fn list_box_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let mut world = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let (rows, pivot, selected, scroll_bar, placeholder, bottom_row, column_number) = {
            let l = self.list_box(id);
            (l.rows.clone(), l.pivot, l.selected_item_index, l.scroll_bar, l.draw_scrollbar_placeholder, l.bottom_row, l.column_number)
        };
        for row in &rows {
            let i = self.list_box_row(*row).id - rows.len() as i32 + pivot;
            let item = if i >= 0 { self.list_box(id).items.get(i as usize).cloned() } else { None };
            if let Some(item) = &item {
                self.list_box_row_set_row(*row, item);
            }
            if i == selected {
                self.controls[*row].draw_background = true;
                self.controls[*row].back_color = [1.0, 0.0, 0.0, 0.6];
            } else {
                self.controls[*row].draw_background = false;
                let f = if i % 2 == 1 { 1.0 } else { 0.5 };
                if let Some(item) = &item {
                    let b = item.back_color;
                    self.list_box_row_mut(*row).row_background = [f * b[0], f * b[1], f * b[2], b[3]];
                }
            }
        }
        let gl = self.controls[id].gl;
        let count = self.list_box(id).items.len() as u64;
        if count <= self.scroll_bar(scroll_bar).min_value as u64 {
            self.set_visible(scroll_bar, false);
            if placeholder {
                painter.gl_color4f(gl, 0.75, 0.75, 0.75, 0.75);
                let s = self.controls[scroll_bar].rect;
                let w = self.local_to_world(id, [s.left, s.top]);
                painter.gl_quad(gl, w[0], w[1], s.right - s.left, s.bottom - s.top, false);
            }
        } else {
            self.set_visible(scroll_bar, true);
        }
        self.control_render(id, dt, painter);
        painter.gl_color4f(gl, 0.85, 0.85, 0.85, 0.8);
        let r = self.controls[id].rect;
        let mut h = r.bottom - r.top;
        if self.controls[bottom_row].visible {
            let b = self.controls[bottom_row].rect;
            h += b.bottom - b.top;
        }
        for c in 1..column_number {
            let s = self.controls[scroll_bar].rect;
            let x = (((r.right - r.left) - (s.right - s.left)) / column_number as f32) * c as f32 + world.left;
            painter.gl_quad(gl, x, world.top, 2.0, h, false);
        }
    }

    // ----- Graph -----

    /// `Graph::Graph` 0x140247170.
    pub fn new_graph(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.set_widget3(id, Widget3::Graph(Graph { min_y: -1.0, max_y: 1.0, max_values_count: 100, auto_adjust_max_value: false, series: Vec::new(), axes: Vec::new() }));
        id
    }

    pub fn graph(&self, id: Id) -> &Graph {
        match self.widget3(id) {
            Widget3::Graph(g) => g,
            _ => panic!("control {id} is not a graph"),
        }
    }

    pub fn graph_mut(&mut self, id: Id) -> &mut Graph {
        match self.widget3_mut(id) {
            Widget3::Graph(g) => g,
            _ => panic!("control {id} is not a graph"),
        }
    }

    /// `Graph::addSerie` 0x140247620.
    pub fn graph_add_serie(&mut self, id: Id, color: [f32; 3]) {
        self.graph_mut(id).series.push(ValueSerie { color, values: Vec::new() });
    }

    /// `Graph::addValue` 0x140247700: with `autoAdjustMaxValue` the top of the range grows.
    pub fn graph_add_value(&mut self, id: Id, serie: u32, value: f32) {
        let graph = self.graph_mut(id);
        if serie as usize >= graph.series.len() {
            return;
        }
        if graph.auto_adjust_max_value && value > graph.max_y {
            graph.max_y = value;
        }
        graph.series[serie as usize].values.push(value);
    }

    /// `Graph::clearSerie` 0x140247760.
    pub fn graph_clear_serie(&mut self, id: Id, serie: u32) {
        if let Some(serie) = self.graph_mut(id).series.get_mut(serie as usize) {
            serie.values.clear();
        }
    }

    /// `Graph::getValuesAtPercentage` 0x1402477d0 (the game reads past the end for 1.0).
    pub fn graph_value_at_percentage(&self, id: Id, serie: usize, percentage: f32) -> Option<f32> {
        let values = &self.graph(id).series.get(serie)?.values;
        values.get((values.len() as f32 * percentage) as i64 as u32 as usize).copied()
    }

    /// `Graph::render` 0x1402478e0 with `renderAxes` 0x140247ba0: a point a pixel column, the
    /// value at the column's share of the window.
    fn graph_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let graph = self.graph_mut(id);
        let max = graph.max_values_count as usize;
        for serie in &mut graph.series {
            while serie.values.len() > max {
                serie.values.remove(0);
            }
        }
        let gl = match self.graph_gl {
            Some(gl) => gl,
            None => {
                let gl = painter.create_gl(GRAPH_GL_VERTICES);
                self.graph_gl = Some(gl);
                gl
            }
        };
        let r = self.controls[id].rect;
        let (width, height) = (r.right - r.left, r.bottom - r.top);
        for axis in self.graph(id).axes.clone() {
            painter.gl_begin(gl, GL_LINES);
            painter.gl_color3f(gl, axis.color[0], axis.color[1], axis.color[2]);
            let (p1, p2) = if axis.is_vertical {
                let x = width * axis.ref_value;
                (self.local_to_world(id, [x, 0.0]), self.local_to_world(id, [x, r.bottom - r.top]))
            } else {
                let y = height * axis.ref_value;
                (self.local_to_world(id, [0.0, y]), self.local_to_world(id, [r.right - r.left, y]))
            };
            painter.gl_vertex3f(gl, p1[0], p1[1], 0.0);
            painter.gl_vertex3f(gl, p2[0], p2[1], 0.0);
            painter.gl_end(gl);
        }
        let graph = self.graph(id);
        let scale = graph.max_values_count as f32 / (r.right - r.left);
        let (min_y, max_y) = (graph.min_y, graph.max_y);
        let pixel_width = (r.right - r.left) as i32 as u32;
        for s in 0..graph.series.len() {
            let serie = &self.graph(id).series[s];
            if serie.values.len() <= 3 {
                continue;
            }
            let (color, values) = (serie.color, serie.values.clone());
            painter.gl_color3f(gl, color[0], color[1], color[2]);
            let mut column = 0u32;
            let mut done = false;
            if pixel_width == 0 {
                continue;
            }
            loop {
                painter.gl_begin(gl, GL_LINE_STRIP);
                let mut n = 0;
                while n < GRAPH_GL_VERTICES {
                    let x = column as f32;
                    let index = (x * scale) as i64 as u32 as usize;
                    if index >= values.len() {
                        done = true;
                        break;
                    }
                    let mut t = (values[index] - min_y) / (max_y - min_y);
                    if t > 1.0 {
                        t = 1.0;
                    } else if !(t >= 0.0) {
                        t = 0.0;
                    }
                    let y = (r.bottom - r.top) * (1.0 - t);
                    let p = self.local_to_world(id, [x, y]);
                    painter.gl_vertex3f(gl, p[0], p[1], 0.0);
                    n += 1;
                    column += 1;
                }
                painter.gl_end(gl);
                if done {
                    break;
                }
                column -= 1;
                if column >= pixel_width {
                    break;
                }
            }
        }
    }

    // ----- PitStopFuel -----

    /// `PitStopFuel::PitStopFuel` 0x1400a7460: no frame, no background, the gauge's picture.
    pub fn new_pit_stop_fuel(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].back_texture = painter.get_texture("content/gui/pitstop/fuel_meter.png");
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        self.set_widget3(id, Widget3::PitStopFuel(PitStopFuel::default()));
        id
    }

    pub fn pit_stop_fuel_mut(&mut self, id: Id) -> &mut PitStopFuel {
        match self.widget3_mut(id) {
            Widget3::PitStopFuel(f) => f,
            _ => panic!("control {id} is not a fuel gauge"),
        }
    }

    /// `PitStopFuel::render` 0x1400aa1d0: the picture, then the tank in green and what is to
    /// be added in red after it (on `GraphicsManager::gl`).
    fn pit_stop_fuel_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let Widget3::PitStopFuel(f) = self.widget3(id) else { return };
        let gl = crate::painter::GRAPHICS_GL;
        painter.gl_color4f(gl, 0.0, 1.0, 0.0, 0.7);
        painter.gl_quad(gl, r.left, r.top, (r.right - r.left) * f.fuel_percentage, r.bottom - r.top, false);
        painter.gl_color4f(gl, 1.0, 0.0, 0.0, 0.7);
        painter.gl_quad(gl, (r.right - r.left) * f.fuel_percentage + r.left, r.top, (r.right - r.left) * f.requested, r.bottom - r.top, false);
    }

    /// `PitStopFuel::onMouseDown` 0x1400a9ce0: no children, no click event; always true.
    fn pit_stop_fuel_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        let mut r = self.controls[id].rect;
        let width = r.right - r.left;
        self.get_world_rect(id, &mut r);
        if let Some(share) = rustyac_physics::race::pitstop::fuel_gauge_share([r.left, r.right, r.top, r.bottom], width, e.x, e.y) {
            self.pit_stop_fuel_mut(id).clicked = Some(share);
        }
        true
    }

    // ----- the virtual functions, by widget -----

    pub fn widget3_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        match self.widget3(id) {
            Widget3::ListBoxRow(_) => self.list_box_row_render(id, dt, painter),
            Widget3::ListBox(_) => self.list_box_render(id, dt, painter),
            Widget3::Graph(_) => self.graph_render(id, dt, painter),
            Widget3::Spinner(_) => self.spinner_render(id, dt, painter),
            Widget3::CustomSpinner(_) => self.custom_spinner_render(id, dt, painter),
            Widget3::PitStopFuel(_) => self.pit_stop_fuel_render(id, dt, painter),
        }
    }

    pub fn widget3_set_size(&mut self, id: Id, w: f32, h: f32) {
        match self.widget3(id) {
            Widget3::ListBoxRow(row) => {
                let columns = row.columns.clone();
                self.control_set_size(id, w, h);
                let count = columns.len() as f32;
                for column in columns {
                    self.set_size(column, w / count, h);
                }
            }
            Widget3::ListBox(_) => self.list_box_set_size(id, w, h),
            Widget3::Spinner(_) => self.spinner_set_size(id, w, h),
            Widget3::CustomSpinner(_) => self.custom_spinner_set_size(id, w, h),
            _ => self.control_set_size(id, w, h),
        }
    }

    pub fn widget3_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        match self.widget3(id) {
            Widget3::Spinner(_) => self.spinner_on_mouse_down(id, e),
            Widget3::PitStopFuel(_) => self.pit_stop_fuel_on_mouse_down(id, e),
            _ => self.control_on_mouse_down(id, e),
        }
    }

    pub fn widget3_on_mouse_up(&mut self, id: Id, e: &MouseEvent) {
        self.control_on_mouse_up(id, e);
    }

    pub fn widget3_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        match self.widget3(id) {
            Widget3::Spinner(_) => self.spinner_on_mouse_move(id, e),
            _ => self.control_on_mouse_move(id, e),
        }
    }

    pub fn widget3_on_mouse_wheel(&mut self, id: Id, e: &MouseEvent) {
        match self.widget3(id) {
            Widget3::ListBox(l) => {
                // the wheel over the list turns its scroll bar, and nothing else of it
                let scroll_bar = l.scroll_bar;
                let mut r = self.controls[id].rect;
                self.get_world_rect(id, &mut r);
                let (x, y) = (e.x as f32, e.y as f32);
                if r.left < x && x < (r.right - r.left) + r.left && r.top < y && y < (r.bottom - r.top) + r.top {
                    self.on_mouse_wheel(scroll_bar, e);
                }
            }
            _ => self.control_on_mouse_wheel(id, e),
        }
    }

    pub fn widget3_on_key_char(&mut self, id: Id, key: u32) {
        self.control_on_key_char(id, key);
    }

    pub fn widget3_on_key_down(&mut self, id: Id, key: u32) {
        self.control_on_key_down(id, key);
    }

    pub fn widget3_set_text(&mut self, id: Id, text: &str) {
        self.controls[id].text = text.to_string();
    }

    pub fn widget3_scale_by_mult(&mut self, id: Id, m: f32) {
        match self.widget3(id) {
            Widget3::Spinner(_) => self.spinner_scale_by_mult(id, m),
            Widget3::CustomSpinner(_) => self.custom_spinner_scale_by_mult(id, m),
            _ => self.control_scale_by_mult(id, m),
        }
    }

    pub fn widget3_set_repeat_interval(&mut self, id: Id, v: f32) {
        match self.widget3(id) {
            Widget3::Spinner(_) => self.spinner_set_repeat_interval(id, v),
            _ => self.controls[id].repeat_interval = v,
        }
    }
}
