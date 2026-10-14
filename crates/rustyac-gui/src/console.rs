// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `Console` (0x128 bytes), the developers' console (Home): a table of commands and one of
//! variables, the input line with its history, the last 12 lines of output, drawn last of the
//! HUD objects. Every text is English (no `acTranslate`).
//!
//! `Console::Console` 0x1401888c0, `operator<<` 0x1401894b0 / 0x140189280 / 0x1401890e0 /
//! 0x140188f40, `addCommand` 0x14018a530, `addVar` 0x14018a6c0, `addVarLambda` 0x14018a830,
//! `initCommands` 0x14018aa80, `show` 0x14018c7f0, `onKeyChar` 0x14018b480, `onKeyDown`
//! 0x14018b620, `parse` 0x14018bf50, `renderHUD` 0x14018c550, `onSetVar` 0x14018b6e0;
//! `ConsoleSet::execute` 0x14018c8f0.
//!
//! A variable's target lives in the game's objects: here a `set` is left in
//! [`Console::actions`] for whoever runs the console to carry out, in the order the game
//! writes them; the console prints what the game prints.

use crate::acos::format_g6;
use crate::font::{Font, FontType};
use crate::painter::{Painter, ALIGN_LEFT, GRAPHICS_GL};

/// `GLRenderer`'s `eQuads`.
const GL_QUADS: i32 = 3;

/// How `set` reaches a variable (`SVar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    /// a `float*` written directly (`set` prints the value)
    Target,
    /// `IVarCallback::onSetVar` (the console's own: the options and the few names it knows)
    Callback,
    /// a lambda (`set` prints the value)
    Lambda,
}

/// `SVar`.
#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub name: String,
    pub kind: VarKind,
    pub multiplier: f32,
}

/// `ConsoleCommand`: its name and help text.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub name: String,
    pub help: String,
}

/// What the console asks of the game.
#[derive(Clone, Debug, PartialEq)]
pub enum ConsoleAction {
    /// a variable set: its number in [`Console::vars`], its name, the value (times its
    /// multiplier)
    Set { index: usize, name: String, value: f32 },
    /// a command other than `set` run with the whole line: its number in
    /// [`Console::commands`] (several may share a name: every one runs)
    Run { index: usize, name: String, line: String },
    /// `exit`: `Game::shutdown`
    Exit,
}

/// `Console`.
pub struct Console {
    /// `isActive`: shown
    pub shown: bool,
    pub vars: Vec<Var>,
    /// the scroll-back (12 lines at most)
    pub lines: Vec<String>,
    pub history: Vec<String>,
    pub history_pos: u32,
    pub font: Font,
    pub command_line: String,
    pub commands: Vec<Command>,
    /// the line being built by `operator<<`
    pub accumulator: String,
    /// `OptionsManager::options` (`Sim+0xf8`): name, value
    pub options: Vec<(String, f32)>,
    pub actions: Vec<ConsoleAction>,
}

/// `OptionsManager::OptionsManager` 0x14018d470 with `loadOptions` 0x14018da20: its four
/// options and their defaults, each read from `system/cfg/options.ini [OPTIONS]` when the file
/// is there (a missing key is 0 then).
pub fn default_options(ini: Option<&rustyac_physics::data::ini::IniReader>) -> Vec<(String, f32)> {
    let mut options = vec![("PROVIEW_MODE".to_string(), 0.0f32), ("AUTOFLIP_RECOVERY".to_string(), 1.0), ("RENDER_SPLINE".to_string(), 0.0), ("DISABLE_SHOW_MODE".to_string(), 0.0)];
    if let Some(ini) = ini {
        for (name, value) in &mut options {
            *value = ini.get_float("OPTIONS", name).unwrap_or(0.0);
        }
    }
    options
}

/// `ksSplitString` 0x1402393b0 (`wcstok_s`): runs of the delimiters make one break, no empty
/// tokens.
fn split(line: &str, delimiters: &[char]) -> Vec<String> {
    line.split(|c| delimiters.contains(&c)).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

impl Console {
    /// `Console::Console(Sim*)` 0x1401888c0.
    pub fn new(painter: &mut dyn Painter) -> Console {
        let mut c = Console {
            shown: false,
            vars: Vec::new(),
            lines: Vec::new(),
            history: Vec::new(),
            history_pos: 0,
            font: Font::new(painter, "", FontType::Monospaced, 16.0, false, false),
            command_line: String::new(),
            commands: Vec::new(),
            accumulator: String::new(),
            options: Vec::new(),
            actions: Vec::new(),
        };
        for line in ["AssettoCorsa, Kunos Simulazioni", "Renderer: KS DX11 Renderer", "COMMAND CONSOLE"] {
            c.print(line);
            c.print("\n");
        }
        c
    }

    /// `operator<<(std::wstring)` 0x1401894b0: "\n" alone ends the line; 12 lines are kept.
    pub fn print(&mut self, s: &str) {
        if s == "\n" {
            self.lines.push(std::mem::take(&mut self.accumulator));
        } else {
            self.accumulator.push_str(s);
        }
        if self.lines.len() > 12 {
            self.lines.remove(0);
        }
    }

    /// `operator<<(float const&)` 0x1401890e0: `%g`, six digits.
    pub fn print_float(&mut self, v: f32) {
        self.accumulator.push_str(&format_g6(v));
    }

    /// `operator<<(int const&)` 0x140188f40.
    pub fn print_int(&mut self, v: i32) {
        self.accumulator.push_str(&v.to_string());
    }

    /// `Console::addVar` / `addVarLambda`.
    pub fn add_var(&mut self, name: &str, kind: VarKind, multiplier: f32) {
        self.vars.push(Var { name: name.to_string(), kind, multiplier });
    }

    /// `Console::addCommand` (and the commands `initCommands` pushes itself).
    pub fn add_command(&mut self, name: &str, help: &str) {
        self.commands.push(Command { name: name.to_string(), help: help.to_string() });
    }

    /// `Console::initCommands` 0x14018aa80 (the first car's arrival): its variables, the
    /// options of `OptionsManager` (in the map's order, by name), `set`, `refresh_instruments`,
    /// `testCrash`, `saveff`.
    pub fn init_commands(&mut self, options: &[(String, f32)]) {
        use VarKind::*;
        self.add_var("fov", Target, 1.0);
        self.add_var("aniso", Callback, 1.0);
        for n in ["driverEye.x", "driverEye.y", "driverEye.z"] {
            self.add_var(n, Target, 1.0);
        }
        for n in ["graphicsOffset.x", "graphicsOffset.y", "graphicsOffset.z", "graphicsPitchRotation", "bias0", "bias1", "bias2"] {
            self.add_var(n, Callback, 1.0);
        }
        self.add_var("ffMult", Lambda, f32::from_bits(0x3a83_126f));
        for n in ["observeDigital", "observeFlames", "observePanel", "observeLights", "maxLayer"] {
            self.add_var(n, Target, 1.0);
        }
        let mut options = options.to_vec();
        options.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
        for (name, _) in &options {
            self.add_var(name, Callback, 1.0);
        }
        self.options = options;
        self.add_command("set", "usage: 'set [var] [value]'");
        self.add_command("refresh_instruments", "refresh_instruments: no parameters needed");
        self.add_command("testCrash", "Test Crash debug");
        self.add_command("saveff", "saves current ffMult value in car.ini");
    }

    /// `Console::show` 0x14018c7f0.
    pub fn show(&mut self, shown: bool) {
        self.shown = shown;
    }

    /// `Console::onKeyChar` 0x14018b480 (a WM_CHAR code; only its low byte, sign-extended).
    pub fn on_key_char(&mut self, key: u32) {
        if !self.shown {
            return;
        }
        if key == 8 {
            let mut units: Vec<u16> = self.command_line.encode_utf16().collect();
            if !units.is_empty() {
                units.pop();
                self.command_line = String::from_utf16_lossy(&units);
            }
        } else if key == 13 {
            let line = self.command_line.clone();
            self.print(&line);
            self.print("\n");
            self.parse(&line);
            self.history.push(line);
            self.history_pos = self.history.len() as u32;
            self.command_line.clear();
        } else {
            let unit = (key as u8 as i8) as i16 as u16;
            let mut units: Vec<u16> = self.command_line.encode_utf16().collect();
            units.push(unit);
            self.command_line = String::from_utf16_lossy(&units);
        }
    }

    /// `Console::onKeyDown` 0x14018b620: Up and Down through the history.
    pub fn on_key_down(&mut self, key: u32) {
        if !self.shown {
            return;
        }
        let len = self.history.len() as u32;
        if key == 0x26 {
            self.history_pos = self.history_pos.wrapping_sub(1);
            if self.history_pos < len {
                self.command_line = self.history[self.history_pos as usize].clone();
            } else {
                self.history_pos = 0;
            }
        }
        if key == 0x28 {
            self.history_pos = self.history_pos.wrapping_add(1);
            if self.history_pos < len {
                self.command_line = self.history[self.history_pos as usize].clone();
            } else {
                self.history_pos = len.wrapping_sub(1);
            }
        }
    }

    /// `Console::parse` 0x14018bf50.
    pub fn parse(&mut self, line: &str) {
        let tokens = split(line, &[' ', '\n', '\r']);
        let Some(first) = tokens.first() else { return };
        if first == "help" {
            self.print("[ AVAILABLE COMMANDS ]");
            self.print("\n");
            for c in self.commands.clone() {
                self.print("- ");
                self.print(&c.name);
                self.print(" <");
                self.print(&c.help);
                self.print(">");
                self.print("\n");
            }
        } else if first == "exit" {
            self.actions.push(ConsoleAction::Exit);
        } else {
            for (index, c) in self.commands.clone().iter().enumerate() {
                if &c.name == first {
                    if c.name == "set" {
                        self.set_execute(line);
                    } else {
                        self.actions.push(ConsoleAction::Run { index, name: c.name.clone(), line: line.to_string() });
                    }
                }
            }
        }
    }

    /// The number `std::stof` / `wcstod` reads (none: the game's call throws; nothing is done
    /// here).
    fn number(text: &str) -> Option<f32> {
        let p = rustyac_math::wcstod(text);
        if p.consumed == 0 || p.out_of_range {
            None
        } else {
            Some(p.value as f32)
        }
    }

    /// `ConsoleSet::execute` 0x14018c8f0.
    pub fn set_execute(&mut self, line: &str) {
        let t = split(line, &[' ']);
        if t.len() < 2 || t[1] != "help" {
            if t.len() > 2 {
                for (index, v) in self.vars.clone().iter().enumerate() {
                    if t[1] != v.name {
                        continue;
                    }
                    let Some(x) = Self::number(&t[2]) else { return };
                    let value = x * v.multiplier;
                    match v.kind {
                        VarKind::Target | VarKind::Lambda => {
                            self.actions.push(ConsoleAction::Set { index, name: v.name.clone(), value });
                            for s in ["[ ", t[1].as_str(), " = ", t[2].as_str(), " ]", "\n"] {
                                self.print(s);
                            }
                        }
                        VarKind::Callback => self.on_set_var(index, value),
                    }
                    return;
                }
            }
            self.print("[error: set var is unknown]");
            self.print("\n");
        } else {
            self.print("[ HELP FOR SET ]");
            self.print("\n");
            let mut n = 0;
            for v in self.vars.clone() {
                self.print(" -");
                self.print(&v.name);
                self.print("     ");
                if n == 8 {
                    self.print("\n");
                    n = 0;
                } else {
                    n += 1;
                }
            }
            self.print("\n");
        }
    }

    /// `Console::onSetVar` 0x14018b6e0: an option of `OptionsManager`, the car's mesh offset
    /// and pitch, the shadow map's bias (with its line).
    fn on_set_var(&mut self, index: usize, x: f32) {
        let name = self.vars[index].name.clone();
        if let Some(o) = self.options.iter_mut().find(|o| o.0 == name) {
            o.1 = x;
            self.actions.push(ConsoleAction::Set { index, name, value: x });
            return;
        }
        match name.as_str() {
            "graphicsOffset.x" | "graphicsOffset.y" | "graphicsOffset.z" => self.actions.push(ConsoleAction::Set { index, name, value: x }),
            "graphicsPitchRotation" => self.actions.push(ConsoleAction::Set { index, name, value: x * f32::from_bits(0x3c8e_f998) }),
            "bias0" | "bias1" | "bias2" => {
                self.actions.push(ConsoleAction::Set { index, name: name.clone(), value: x });
                self.print(&format!("[SHADOW BIAS{} set to: ", &name[4..]));
                self.print_float(x);
                self.print("]");
                self.print("\n");
            }
            _ => {}
        }
    }

    /// `Console::renderHUD` 0x14018c550: a grey panel 300 px high over the window's width, the
    /// lines, the prompt with its cursor in the odd seconds of the game's clock.
    pub fn render_hud(&mut self, painter: &mut dyn Painter, width: i32, now_ms: f64) {
        if !self.shown {
            return;
        }
        let gl = GRAPHICS_GL;
        let w = width as f32;
        painter.gl_begin(gl, GL_QUADS);
        painter.gl_color3f(gl, f32::from_bits(0x3ecc_cccd), f32::from_bits(0x3ecc_cccd), f32::from_bits(0x3ecc_cccd));
        painter.gl_vertex3f(gl, 0.0, 0.0, 0.0);
        painter.gl_vertex3f(gl, 0.0, 300.0, 0.0);
        painter.gl_vertex3f(gl, w, 300.0, 0.0);
        painter.gl_vertex3f(gl, w, 0.0, 0.0);
        painter.gl_end(gl);
        let mut y = 0.0f32;
        for line in self.lines.clone() {
            self.font.blit_string(painter, 0.0, y, &line, 1.0, ALIGN_LEFT);
            y += 20.0;
        }
        let p = now_ms * 0.001;
        let secs = if p.is_nan() || p >= 2_147_483_648.0 || p <= -2_147_483_649.0 { i32::MIN } else { p as i32 };
        let mut s = format!("ACC> {}", self.command_line);
        if secs % 2 != 0 {
            s.push('_');
        }
        self.font.blit_string(painter, 0.0, 260.0, &s, 1.0, ALIGN_LEFT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(c: &mut Console, text: &str) {
        for ch in text.chars() {
            c.on_key_char(ch as u32);
        }
        c.on_key_char(13);
    }

    fn bare() -> Console {
        Console { shown: true, vars: Vec::new(), lines: Vec::new(), history: Vec::new(), history_pos: 0, font: Font { scale: 16.0, shadowed: false, shadow_pixel_distance: 2.0, kid: 0, color: 0xffff_ffff, current_alpha: 1.0 }, command_line: String::new(), commands: Vec::new(), accumulator: String::new(), options: Vec::new(), actions: Vec::new() }
    }

    #[test]
    fn set_help_and_history() {
        let mut c = bare();
        c.init_commands(&[("PROVIEW_MODE".into(), 0.0), ("AUTOFLIP_RECOVERY".into(), 1.0)]);
        keys(&mut c, "set fov 60");
        keys(&mut c, "set ffMult 1000");
        keys(&mut c, "set nothing 1");
        keys(&mut c, "set AUTOFLIP_RECOVERY 0");
        assert_eq!(c.lines, ["set fov 60", "[ fov = 60 ]", "set ffMult 1000", "[ ffMult = 1000 ]", "set nothing 1", "[error: set var is unknown]", "set AUTOFLIP_RECOVERY 0"]);
        assert_eq!(c.actions[1], ConsoleAction::Set { index: 12, name: "ffMult".into(), value: 1000.0 * f32::from_bits(0x3a83_126f) });
        assert_eq!(c.options[0], ("AUTOFLIP_RECOVERY".to_string(), 0.0));
        c.on_key_down(0x26);
        assert_eq!(c.command_line, "set AUTOFLIP_RECOVERY 0");
        c.on_key_down(0x28);
        assert_eq!(c.command_line, "set AUTOFLIP_RECOVERY 0");
        c.command_line.clear();
        keys(&mut c, "help");
        assert_eq!(c.lines.len(), 12);
        assert_eq!(c.lines.last().unwrap(), "- saveff <saves current ffMult value in car.ini>");
    }
}
