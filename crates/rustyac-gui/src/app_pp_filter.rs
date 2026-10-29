// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `PostProcessFilterSelector` (0x1c8), "Post Process Filter": a spinner of the post-processing
//! filters (`ppfilters\*.ini`). Picking one asks for it (`loadConfigurationFile`, YEBIS'); the
//! one showing at the game's end goes into video.ini `[POST_PROCESS] FILTER`.
//!
//! Ctor 0x140329360, dtor 0x1403299d0, the spinner's handler 0x140329b10.

use std::any::Any;
use std::path::Path;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::gui::Gui;
use crate::painter::Painter;
use crate::widgets::WidgetEvent;

pub struct PpFilterSelector {
    spinner: Id,
}

/// `Path::getFiles(dir\*.ini)` + `getFileNameWithoutExtension`: the folder's order (NTFS'),
/// directories skipped.
pub fn filter_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
        .map(|e| Path::new(&e.file_name()).to_path_buf())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ini")))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect()
}

impl Gui {
    /// `PostProcessFilterSelector::PostProcessFilterSelector(Sim*)`: `documents` and `system` the
    /// two folders' filters (Documents' first), `default` video.ini's `[POST_PROCESS] FILTER`.
    pub fn new_pp_filter_app(&mut self, painter: &mut dyn Painter, documents: &[String], system: &[String], default: &str) -> Id {
        let form = self.new_form(painter, "POST_PROCESS_FILTER_SELECTOR", true);
        self.form_data_mut(form).dev_app = false;
        self.form_set_title(form, "Post Process Filter");
        let spinner = self.new_spinner(painter, "Filters", true, false);
        for name in documents.iter().chain(system) {
            self.spinner_add_item(spinner, name);
        }
        self.spinner_mut(spinner).handlers.push(Rc::new(|gui: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev else { return };
            let name = gui.spinner_current_item(*spinner);
            gui.apps.actions.push(AppAction::LoadPostProcessFilter(name));
        }));
        self.set_size(spinner, 200.0, 30.0);
        self.spinner_set_position(spinner, 58.0, 70.0);
        self.add_control(form, spinner);
        // the game's quirk: only the first (system folder's count) items are looked at
        for i in 0..system.len() {
            if self.spinner_mut(spinner).items.get(i).is_some_and(|item| item == default) {
                self.spinner_set_value(spinner, i as i32);
            }
        }
        self.form_set_size(form, 300.0, 110.0);
        // Form::scaleByMult() 0x140246a10: by its own scaleMult
        let scale = self.controls[form].scale_mult;
        self.form_scale_by_mult(form, scale);
        self.set_app(form, Box::new(PpFilterSelector { spinner }));
        form
    }
}

impl App for PpFilterSelector {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// The dtor (the game's end): the filter showing into video.ini.
    fn shutdown(&mut self, gui: &mut Gui, _id: Id) {
        let value = gui.spinner_current_item(self.spinner);
        gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/video.ini".into(), section: "POST_PROCESS".into(), key: "FILTER".into(), value });
    }
}
