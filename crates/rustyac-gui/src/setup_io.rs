// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupIOManager` (0x270 bytes), the setup editor's page "Setup I/O": the list of tracks
//! ("generic" first), the setups saved for the chosen one, and Load / Reset To Default /
//! Delete / Save.
//!
//! Constructor 0x140174c90, `showStatus` 0x140178740, `cutStringIfExceeds` 0x140176f60,
//! `onVisibleChanged` 0x140177270, `setSelectedTrack` 0x140178730, `resetUI` 0x140177de0,
//! `setOverrideStatus` 0x140178480, `setRemoveStatus` 0x140178620, `refreshStatus`
//! 0x1401776d0, `getPathFromSetupName` 0x140177040, `refreshSetupListBox` 0x140177290,
//! `render` 0x1401779d0, `sendSaveMessage` 0x140177f20, the handlers of the buttons (load
//! 0x1401768e0, reset 0x140176cd0, delete 0x140176c90, its confirmation 0x140176660) and of
//! the setups' list (0x1401767f0).
//!
//! The game has one folder, `Documents\Assetto Corsa\setups`. Here there are two
//! ([`crate::setup_car::SetupLink`]): rustyAC's own, where everything is written (the folders
//! the constructor makes, Save, Delete), and optionally the game's, which is only read: its
//! setups are listed after rustyAC's own, marked " (AC)", and can be loaded.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_CENTER, ALIGN_LEFT};
use crate::setup_tab::TabKind;
use crate::widgets::WidgetEvent;
use crate::widgets3::ListBoxRowData;

const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
/// What a setup of the game's own folder has after its name in the list.
pub const AC_MARK: &str = " (AC)";

/// `SetupIOManager`'s own members.
pub struct SetupIo {
    /// the `SetupScreen` (the game finds it through `Sim::escMenu`)
    pub screen: Id,
    pub override_setup: bool,
    pub track_list_box: Id,
    pub setup_list_box: Id,
    pub reset_button: Id,
    pub load_button: Id,
    pub save_button: Id,
    pub remove_button: Id,
    pub confirm_button: Id,
    pub text_input: Id,
    pub active_setup: String,
    pub active_track: String,
    pub status: String,
    pub status_timer: f32,
    /// the folder each row of the setups' list is in
    pub setup_folders: Vec<PathBuf>,
}

/// `ListBoxRowData::ListBoxRowData(name)` 0x14006eed0.
pub fn row_data(name: &str) -> ListBoxRowData {
    ListBoxRowData { name: name.to_string(), category: 0, back_color: [0.5, 0.5, 0.5, 0.7], forecolors: vec![[1.0; 4]], columns: vec![name.to_string()] }
}

/// `cutStringIfExceeds` 0x140176f60.
pub fn cut_string_if_exceeds(text: &str, n: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if n < units.len() {
        format!("{}...", String::from_utf16_lossy(&units[..n.saturating_sub(3)]))
    } else {
        text.to_string()
    }
}

/// `Path::doesFilenameContainsIllegalChar` 0x140230520.
pub fn filename_contains_illegal_char(name: &str) -> bool {
    name.chars().any(|c| "\\/:?\"<>|*%$".contains(c))
}

/// `Path::getFiles(folder\*.ini)` 0x140230b00, each name cut at its last dot: the folder's
/// order is the file system's (by name, whatever the case).
fn setup_names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.retain(|name| name.to_ascii_lowercase().ends_with(".ini"));
    names.sort_by_key(|name| name.to_uppercase().encode_utf16().collect::<Vec<u16>>());
    names.into_iter().map(|name| name.rfind('.').map_or(name.clone(), |dot| name[..dot].to_string())).collect()
}

impl Gui {
    fn setup_io(&self, id: Id) -> &SetupIo {
        match &self.setup_tab(id).kind {
            TabKind::Io(io) => io,
            _ => panic!("control {id} is not the setup I/O page"),
        }
    }

    fn setup_io_mut(&mut self, id: Id) -> &mut SetupIo {
        match &mut self.setup_tab_mut(id).kind {
            TabKind::Io(io) => io,
            _ => panic!("control {id} is not the setup I/O page"),
        }
    }

    fn setup_io_button(&mut self, painter: &mut dyn Painter, name: &str, texture: &str) -> Id {
        let button = self.new_active_button(painter, name);
        self.set_multiple_texture(painter, button, texture);
        button
    }

    fn setup_io_list(&mut self, painter: &mut dyn Painter, tab: Id, name: &str, x: f32) -> Id {
        let list = self.new_list_box(painter, name, 15, 1);
        self.add_control(tab, list);
        self.set_position(list, x, 110.0);
        self.set_size(list, 202.0, 290.0);
        self.list_box_mut(list).allow_deselection = false;
        list
    }

    fn setup_io_list_title(&mut self, list: Id, title: &str) {
        self.controls[list].back_color = [0.15, 0.15, 0.15, 0.8];
        self.controls[list].border_color = [0.2, 0.2, 0.2, 0.8];
        let row = self.list_box(list).title_row;
        self.list_box_row_set_row(row, &row_data(&format!("{} : ", translate(title))));
        self.list_box_row_set_font_alignment(row, ALIGN_CENTER);
        self.list_box_row_mut(row).draw_row_background = false;
        self.list_box_draw_cells_backgrounds(list, false);
    }

    /// The folder of a car's setups for a track under a root (`setups\<car>\<track>`).
    fn setup_io_folder(&self, root: &Path, track: &str) -> PathBuf {
        let car = self.setup_car.as_ref().map(|car| car.unix_name()).unwrap_or_default();
        root.join(car).join(track)
    }

    /// `SetupIOManager::SetupIOManager` 0x140174c90.
    pub fn new_setup_io(&mut self, painter: &mut dyn Painter, screen: Id) -> Id {
        let id = self.new_setup_tab(painter, "Setup I/O");
        self.control_set_size(id, 450.0, 600.0);
        let track_list_box = self.setup_io_list(painter, id, "TRACK_LISTBOX", 15.0);
        // the folders the game makes in Documents: here in rustyAC's own folder
        let save_root = self.setup.setups_save.clone();
        let _ = rustyac_content::guard::create_dir_all(self.setup_io_folder(&save_root, "generic"));
        self.list_box_add_item(track_list_box, &row_data("generic"));
        for track in self.setup.tracks.clone() {
            if track == "ai" {
                continue;
            }
            self.list_box_add_item(track_list_box, &row_data(&track));
            let _ = rustyac_content::guard::create_dir(self.setup_io_folder(&save_root, &track));
        }
        self.list_box_set_items_font_size(track_list_box, 15.0);
        self.list_box_scroll_to_top(track_list_box);
        self.setup_io_list_title(track_list_box, "Track");

        let tab = self.controls[id].rect;
        let width = tab.right - tab.left;
        let load_button = self.setup_io_button(painter, "LOAD_BUTTON", "content/gui/setupIO/btn_load");
        self.add_control(id, load_button);
        self.set_size(load_button, 50.0, 50.0);
        let tl = self.controls[track_list_box].rect;
        self.set_position(load_button, tl.left, ((tl.bottom - tl.top) + tl.top) + 10.0);
        self.set_text(load_button, &translate("Load"));
        let r = self.controls[load_button].rect;
        self.active_button_mut(load_button).text_coord = [0.0, r.bottom - r.top];
        let top = r.top;

        let reset_button = self.setup_io_button(painter, "RESET_BUTTON", "content/gui/setupIO/reset_button");
        self.add_control(id, reset_button);
        self.set_size(reset_button, 50.0, 50.0);
        let r = self.controls[reset_button].rect;
        self.set_position(reset_button, (width * 0.5 - (r.right - r.left)) - 45.0, top);
        // reset (0x140176cd0)
        self.controls[reset_button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
            gui.setup_io_reset_ui(id);
            let screen = gui.setup_io(id).screen;
            gui.setup_screen_reset_tabs_value_to_default(screen);
            gui.setup_io_refresh_status(id);
        }));
        self.set_text(reset_button, &translate("Reset To Default"));
        let r = self.controls[reset_button].rect;
        self.active_button_mut(reset_button).text_coord = [0.0, r.bottom - r.top];

        let remove_button = self.setup_io_button(painter, "REMOVE_BUTTON", "content/gui/setupIO/btn_delete");
        self.add_control(id, remove_button);
        self.set_size(remove_button, 50.0, 50.0);
        self.set_position(remove_button, width * 0.5 + 45.0, top);
        self.set_text(remove_button, &translate("Delete"));
        let r = self.controls[remove_button].rect;
        self.active_button_mut(remove_button).text_coord = [0.0, r.bottom - r.top];
        // delete (0x140176c90): asks first
        self.controls[remove_button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
            let list = gui.setup_io(id).setup_list_box;
            if gui.list_box_selected_item(list).is_some() {
                gui.setup_io_set_remove_status(id, true);
            }
        }));

        let confirm_button = self.new_control(painter, "CONFIRM_BUTTON");
        self.add_control(id, confirm_button);
        self.controls[confirm_button].back_color = COL_TITLE_BAR;
        self.set_size(confirm_button, 50.0, 16.0);
        self.set_position(confirm_button, width - 60.0, 570.0);
        self.controls[confirm_button].font_scale = 0.7;
        self.set_text(confirm_button, &translate("DELETE"));
        // the confirmation (0x140176660): the setup's file and its pit strategy's go
        self.controls[confirm_button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| gui.setup_io_delete_selected(id)));

        let save_button = self.setup_io_button(painter, "SAVE_BUTTON", "content/gui/setupIO/btn_save");
        self.set_size(save_button, 50.0, 50.0);
        let r = self.controls[save_button].rect;
        self.set_position(save_button, (width - (r.right - r.left)) - 15.0, top);
        self.set_text(save_button, &translate("Save"));
        let r = self.controls[load_button].rect;
        self.active_button_mut(save_button).text_coord = [0.0, r.bottom - r.top];

        let text_input = self.new_text_input(painter, "SAVE_SETUP_TEXT_INPUT");
        self.set_visible(text_input, true);
        self.controls[text_input].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[text_input].border_color = [0.25, 0.25, 0.25, 0.5];
        self.text_input_mut(text_input).back_color_unfocus = [0.25, 0.25, 0.25, 0.5];
        self.text_input_mut(text_input).back_color_focus = [0.0, 0.0, 0.0, 0.5];
        self.add_control(id, text_input);
        self.set_position(text_input, 15.0, 565.0);
        let r = self.controls[save_button].rect;
        self.set_size(text_input, (width - 30.0) - (r.right - r.left), 25.0);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 17.0, false, false));
        self.controls[text_input].font = Some(self.fonts.len() - 1);
        self.set_text(save_button, &translate("Save"));
        self.set_visible(save_button, true);
        self.controls[save_button].back_color = [0.5, 0.5, 0.5, 0.8];

        let setup_list_box = self.setup_io_list(painter, id, "SETUP_LIST_BOX", 232.0);
        self.setup_io_list_title(setup_list_box, "Setup");

        self.setup_tab_mut(id).kind = TabKind::Io(Box::new(SetupIo {
            screen,
            override_setup: false,
            track_list_box,
            setup_list_box,
            reset_button,
            load_button,
            save_button,
            remove_button,
            confirm_button,
            text_input,
            active_setup: "Default".to_string(),
            active_track: "generic".to_string(),
            status: String::new(),
            status_timer: 0.0,
            setup_folders: Vec::new(),
        }));
        self.set_text(text_input, "");
        self.setup_io_show_status(id, true, &translate("Default loaded"));

        // load (0x1401768e0)
        self.controls[load_button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| gui.setup_io_load_selected(id)));
        self.controls[save_button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| gui.setup_io_send_save_message(id)));
        self.controls[text_input].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| gui.setup_io_reset_ui(id)));
        self.text_input_mut(text_input).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if matches!(ev, WidgetEvent::Text { kind: 0, .. }) {
                gui.setup_io_reset_ui(id);
            }
        }));
        // a setup of the list is chosen (0x1401767f0)
        self.list_box_mut(setup_list_box).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::ListBoxRow { row, kind: 0, .. } = ev {
                let io = gui.setup_io(id);
                let (list, input, load) = (io.setup_list_box, io.text_input, io.load_button);
                let name = gui.list_box(list).items.get(*row as usize).map(|item| item.name.clone()).unwrap_or_default();
                gui.set_text(input, &name);
                gui.set_text(load, &translate("Load"));
                gui.setup_io_reset_ui(id);
            }
        }));
        self.list_box_mut(track_list_box).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if matches!(ev, WidgetEvent::ListBoxRow { kind: 0, .. }) {
                gui.setup_io_refresh_setup_list_box(id);
            }
        }));
        self.add_control(id, save_button);
        id
    }

    /// `SetupIOManager::showStatus` 0x140178740.
    fn setup_io_show_status(&mut self, id: Id, show: bool, message: &str) {
        let io = self.setup_io_mut(id);
        if show {
            io.status_timer = 4.0;
            io.status = message.to_string();
        } else {
            io.status_timer = -1.0;
        }
    }

    /// `SetupIOManager::setSelectedTrack` 0x140178730.
    pub fn setup_io_set_selected_track(&mut self, id: Id, track: &str) {
        let list = self.setup_io(id).track_list_box;
        self.list_box_select_item_by_name(list, track);
    }

    /// `SetupIOManager::resetUI` 0x140177de0.
    fn setup_io_reset_ui(&mut self, id: Id) {
        let io = self.setup_io_mut(id);
        io.status_timer = -1.0;
        io.override_setup = false;
        let (save, confirm) = (io.save_button, io.confirm_button);
        self.set_text(save, &translate("Save"));
        self.controls[save].back_color = [0.5, 0.5, 0.5, 0.8];
        self.set_visible(confirm, false);
    }

    /// `SetupIOManager::setOverrideStatus` 0x140178480.
    fn setup_io_set_override_status(&mut self, id: Id, on: bool) {
        self.setup_io_reset_ui(id);
        if on {
            let io = self.setup_io_mut(id);
            io.status_timer = 4.0;
            io.status = translate("Are you sure to override the setup ?");
            let save = io.save_button;
            self.set_text(save, &translate("Override"));
            self.setup_io_mut(id).override_setup = true;
            self.controls[save].back_color = [1.0, 0.3, 0.3, 1.0];
        }
    }

    /// `SetupIOManager::setRemoveStatus` 0x140178620.
    fn setup_io_set_remove_status(&mut self, id: Id, on: bool) {
        self.setup_io_reset_ui(id);
        if on {
            let io = self.setup_io_mut(id);
            io.status_timer = 4.0;
            io.status = translate("Are you sure to delete the setup ?");
            let confirm = io.confirm_button;
            self.set_visible(confirm, true);
        }
    }

    /// `SetupIOManager::refreshStatus` 0x1401776d0: a star after the setup's name while it has
    /// changes that are not saved.
    fn setup_io_refresh_status(&mut self, id: Id) {
        let screen = self.setup_io(id).screen;
        let need = self.setup_screen_need_to_save(screen);
        let io = self.setup_io_mut(id);
        let starred = io.active_setup.ends_with('*');
        if need {
            io.status = format!("{} : {}", translate("Need to save current setup"), cut_string_if_exceeds(&io.active_setup, 0x17));
            io.status_timer = 4.0;
            if !starred {
                io.active_setup.push('*');
            }
        } else {
            if starred {
                io.active_setup.pop();
            }
            io.status_timer = -1.0;
        }
    }

    /// `SetupIOManager::onVisibleChanged` 0x140177270.
    pub(crate) fn setup_io_on_visible_changed(&mut self, id: Id, visible: bool) {
        if visible {
            self.setup_io_refresh_status(id);
            self.setup_io_reset_ui(id);
        }
    }

    /// The chosen track's name.
    fn setup_io_selected_track(&self, id: Id) -> Option<String> {
        self.list_box_selected_item(self.setup_io(id).track_list_box).map(|row| row.name.clone())
    }

    /// `SetupIOManager::getPathFromSetupName` 0x140177040, for a file of rustyAC's own folder
    /// (none: no name or no track, where the game's answer is no file's path either).
    fn setup_io_save_path(&self, id: Id, name: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        let track = self.setup_io_selected_track(id)?;
        Some(self.setup_io_folder(&self.setup.setups_save, &track).join(format!("{name}.ini")))
    }

    /// `SetupIOManager::refreshSetupListBox` 0x140177290: the `.ini` files of the chosen
    /// track's folder, rustyAC's own first, then the game's (marked).
    fn setup_io_refresh_setup_list_box(&mut self, id: Id) {
        let list = self.setup_io(id).setup_list_box;
        self.list_box_clear(list);
        self.setup_io_mut(id).setup_folders.clear();
        let Some(track) = self.setup_io_selected_track(id) else { return };
        let own = self.setup_io_folder(&self.setup.setups_save, &track);
        for name in setup_names(&own) {
            self.list_box_add_item(list, &row_data(&name));
            self.setup_io_mut(id).setup_folders.push(own.clone());
        }
        if let Some(root) = self.setup.setups_read.clone() {
            let folder = self.setup_io_folder(&root, &track);
            for name in setup_names(&folder) {
                let mut row = row_data(&name);
                row.columns = vec![format!("{name}{AC_MARK}")];
                self.list_box_add_item(list, &row);
                self.setup_io_mut(id).setup_folders.push(folder.clone());
            }
        }
        self.list_box_scroll_to_top(list);
    }

    /// The chosen setup's file.
    fn setup_io_selected_file(&self, id: Id) -> Option<(String, PathBuf, bool)> {
        let io = self.setup_io(id);
        let list = self.list_box(io.setup_list_box);
        let index = usize::try_from(list.selected_item_index).ok()?;
        let (row, folder) = (list.items.get(index)?, io.setup_folders.get(index)?);
        let own = folder.starts_with(&self.setup.setups_save);
        Some((row.name.clone(), folder.join(format!("{}.ini", row.name)), own))
    }

    /// The load button's handler (0x1401768e0).
    fn setup_io_load_selected(&mut self, id: Id) {
        let Some((name, path, _)) = self.setup_io_selected_file(id) else { return };
        self.setup_io_reset_ui(id);
        let screen = self.setup_io(id).screen;
        self.setup_screen_load_setup_absolute_path(screen, &path, false);
        let track = self.setup_io_selected_track(id);
        let io = self.setup_io_mut(id);
        io.status_timer = 4.0;
        io.status = format!("{} : {}", translate("Loaded"), cut_string_if_exceeds(&name, 0x17));
        if let Some(track) = track {
            io.active_track = track;
        }
        io.active_setup = name;
    }

    /// The handler of the delete's confirmation (0x140176660). A setup of the game's own
    /// folder is not rustyAC's to delete: it stays, and the page says so.
    fn setup_io_delete_selected(&mut self, id: Id) {
        let Some((_, path, own)) = self.setup_io_selected_file(id) else { return };
        if !own {
            self.setup_io_reset_ui(id);
            self.setup_io_show_status(id, true, "A setup of Assetto Corsa's folder is not deleted here");
            return;
        }
        if path.is_file() {
            let _ = rustyac_content::guard::remove_file(&path);
        }
        let sp = crate::setup_screen::sp_path(&path);
        if sp.is_file() {
            let _ = rustyac_content::guard::remove_file(&sp);
        }
        self.setup_io_refresh_setup_list_box(id);
        self.setup_io_reset_ui(id);
    }

    /// `SetupIOManager::sendSaveMessage` 0x140177f20: the first click on a name that exists
    /// asks ("Override"), the second writes.
    fn setup_io_send_save_message(&mut self, id: Id) {
        let io = self.setup_io(id);
        let (input, list, screen) = (io.text_input, io.setup_list_box, io.screen);
        let text = self.controls[input].text.clone();
        if text.is_empty() {
            self.setup_io_show_status(id, true, &translate("You must specify a name"));
            return;
        }
        let found = self.list_box_item_id_from_name(list, &text);
        if found >= 0 && self.list_box(list).items[found as usize].category == 2 {
            self.setup_io_show_status(id, true, &translate("You can't overwrite default setup"));
            return;
        }
        if filename_contains_illegal_char(&text) {
            self.setup_io_show_status(id, true, &translate("The name contains illegal characters"));
            return;
        }
        let Some(path) = self.setup_io_save_path(id, &text) else {
            self.setup_io_show_status(id, true, &translate("You must specify a Track first"));
            return;
        };
        if !path.is_file() || self.setup_io(id).override_setup {
            self.setup_screen_save_setup_absolute_path(screen, &path);
            self.setup_io_refresh_setup_list_box(id);
            self.list_box_select_item_by_name(list, &text);
            self.setup_io_reset_ui(id);
            if let Some(name) = self.list_box_selected_item(list).map(|row| row.name.clone()) {
                self.setup_io_mut(id).active_setup = name.clone();
                self.setup_io_show_status(id, true, &format!("{} : {}", translate("Saved"), cut_string_if_exceeds(&name, 0x17)));
            }
            if let Some(track) = self.setup_io_selected_track(id) {
                self.setup_io_mut(id).active_track = track;
            }
            self.set_text(input, "");
        } else {
            self.setup_io_set_override_status(id, true);
        }
    }

    /// `SetupIOManager::render` 0x1401779d0: the page's words are written with the GUI's own
    /// font, which is left at size 15 and (after a status line) yellow.
    pub(crate) fn setup_io_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
        let font = self.font;
        self.controls[id].font = Some(font);
        self.fonts[font].scale = 15.0;
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        let io = self.setup_io(id);
        let (input, text) = (io.text_input, cut_string_if_exceeds(&format!("{} - {}", io.active_track, io.active_setup), 0x23));
        let mut r = self.controls[input].rect;
        self.get_world_rect(input, &mut r);
        if self.setup_io_selected_track(id).is_some() {
            self.fonts[font].blit_string(painter, r.left, r.top - 23.0, &format!("{} : ", translate("Save current setup as")), 1.0, ALIGN_LEFT);
        }
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let cx = (r.right - r.left) * 0.5 + r.left;
        self.fonts[font].blit_string(painter, cx, r.top + 35.0, &format!("{} : ", translate("Current Setup")), 1.0, ALIGN_CENTER);
        self.fonts[font].blit_string(painter, cx, r.top + 60.0, &text, 1.0, ALIGN_CENTER);
        if 0.0 < self.setup_io(id).status_timer {
            self.fonts[font].set_color(1.0, 1.0, 0.0, 1.0);
            let status = self.setup_io(id).status.clone();
            self.fonts[font].blit_string(painter, cx, r.bottom - 90.0, &status, 1.0, ALIGN_CENTER);
            self.setup_io_mut(id).status_timer -= dt;
        }
    }
}
