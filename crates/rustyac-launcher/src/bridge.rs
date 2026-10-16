// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `ACSchemeHandler` and `ACRequestHandler`: the theme's `ac://<handler>/<path>?<query>`
//! requests and their answers, handler for handler as the launcher has them. What a handler
//! leaves to the window (start the game, go home, the controls page, music) comes back as an
//! [`Event`], the launcher's `SchemeEvent`.

use crate::appini;
use crate::folders::{create_if_non_existant, AppFolder};
use crate::ini::IniFile;
use crate::jss::{self, Value};
use crate::net;
use crate::online;
use crate::query::{html_encode, parse_query_string, url_decode, Params};
use crate::sort;
use std::sync::{Arc, Mutex};

/// An exception a handler throws, as `ACSchemeHandler.DoProcess` answers it.
#[derive(Clone, Debug, PartialEq)]
pub enum Thrown {
    ArgumentOutOfRange,
    /// `FileNotFoundException` with its `FileName`.
    FileNotFound(String),
    NullReference,
    /// Any other exception, with its `Message`.
    Other(String),
}

impl Thrown {
    pub(crate) fn answer(&self) -> Answer {
        match self {
            Thrown::ArgumentOutOfRange => Answer::message(&html_encode("A name/value pair is required: ac://setsetting/<inifile>?[<section>/]<name>=<value>")),
            Thrown::FileNotFound(name) => Answer::message(&format!("No such file: {name}")),
            Thrown::NullReference => Answer::message("ERROR\nObject reference not set to an instance of an object."),
            Thrown::Other(m) => Answer::message(&format!("ERROR\n{m}")),
        }
    }
}

/// The .NET exception an I/O error on `path` is.
pub fn io_thrown(path: &str, e: &std::io::Error) -> Thrown {
    match e.kind() {
        std::io::ErrorKind::NotFound => {
            let parent = std::path::Path::new(path).parent();
            if parent.is_some_and(|p| p.is_dir()) {
                Thrown::FileNotFound(net::full_path(path))
            } else {
                dir_missing(path)
            }
        }
        std::io::ErrorKind::PermissionDenied => Thrown::Other(format!("Access to the path '{}' is denied.", net::full_path(path))),
        _ => Thrown::Other(e.to_string()),
    }
}

fn dir_missing(path: &str) -> Thrown {
    Thrown::Other(format!("Could not find a part of the path '{}'.", net::full_path(path)))
}

/// `Directory.GetDirectories` / `GetFiles`, throwing as .NET does for a missing folder.
fn dirs(path: &str) -> Result<Vec<String>, Thrown> {
    net::list(path, true).map_err(|_| dir_missing(path))
}

fn files_with(path: &str, suffix: &str) -> Result<Vec<String>, Thrown> {
    net::files_with(path, suffix).map_err(|_| dir_missing(path))
}

/// `ACRequestData`.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub mime: String,
    pub ok: bool,
    pub body: Vec<u8>,
}

impl Answer {
    fn new(ok: bool, mime: &str, data: &str) -> Answer {
        Answer { mime: mime.into(), ok, body: data.as_bytes().to_vec() }
    }

    /// `ACRequestData(returnValue)`: "OK" as text/html.
    pub(crate) fn plain(ok: bool) -> Answer {
        Answer::new(ok, "text/html", "OK")
    }

    /// `Message(text)`.
    pub(crate) fn message(text: &str) -> Answer {
        Answer::new(true, "text/html", text)
    }

    pub(crate) fn json(text: &str) -> Answer {
        Answer::new(true, "application/json", text)
    }
}

/// What a request asks of the window (`SchemeEvent` with its `FunctionName`), besides the
/// handler's own name, which every request raises.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `start`: race.ini is formatted, the game should start.
    Start,
    Home,
    Exit,
    OpenControls,
    StartMusic,
    StopMusic,
    SetVolume,
    /// `ListCars` / `ListTracks` found content it could not read ("ERRORS with content: ...").
    ContentErrors(String),
}

/// What the window and the Steam calls answer, which the bridge cannot know by itself.
pub trait Host {
    /// `SystemParameters.PrimaryScreenWidth/Height` (device independent pixels).
    fn primary_resolution(&self) -> (f64, f64);
    /// `ACUtility.GetAvailableResolutionsJSON` (EnumDisplaySettings).
    fn display_modes(&self) -> Vec<(u32, u32)>;
    /// `acInitVideoModes` / `acGetVideoMode`: (width, height, refresh).
    fn video_modes(&self) -> Option<Vec<(i32, i32, f32)>>;
    /// The local time of a file's last write (`None`: now), as calendar fields
    /// (year, month, day, hour, minute, second, millisecond).
    fn local_time(&self, file: Option<&str>) -> [i64; 7];
    /// The culture's abbreviated month names (`MMM`).
    fn month_abbrev(&self, month: u32) -> String;
    /// `SteamFriends.GetPersonaName`: rustyAC's player name.
    fn player_name(&self) -> String;
    /// `GetGuid`: the Steam id, else (where the launcher makes a new `Guid.NewGuid()` each
    /// time) rustyAC's own id, the one rustyac.exe sends when it joins.
    fn player_id(&self) -> String;
}

/// The handlers with the state the launcher keeps in static fields.
pub struct Bridge {
    pub folders: AppFolder,
    pub host: Box<dyn Host>,
    generated_css: String,
    car_count: String,
    event_count: String,
    /// Requests whose handler is not ported (they answer "OK"); the shell logs them.
    pub not_ported: Vec<String>,
    pub lobby: Arc<Mutex<online::Lobby>>,
}

/// One request split as `new Uri(url)` has it: host (lower case), `AbsolutePath`, `Query`.
pub fn split_url(url: &str) -> (String, String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.split('#').next().unwrap_or("");
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let host = net::to_lower(&rest[..end]);
    let rest = &rest[end..];
    let (path, query) = match rest.find('?') {
        Some(q) => (&rest[..q], &rest[q..]),
        None => (rest, ""),
    };
    let path = if path.is_empty() { "/" } else { path };
    (host, path.to_string(), query.to_string())
}

fn split_once_slash(s: &str) -> Vec<&str> {
    s.split('/').collect()
}

impl Bridge {
    pub fn new(folders: AppFolder, host: Box<dyn Host>) -> Bridge {
        Bridge { folders, host, generated_css: String::new(), car_count: String::new(), event_count: "0".into(), not_ported: Vec::new(), lobby: Arc::new(Mutex::new(online::Lobby::new(None))) }
    }

    fn params(url: &str, post: Option<&str>) -> (String, String, Params) {
        let (host, path, query) = split_url(url);
        let mut params = parse_query_string(&query);
        if let Some(body) = post {
            params.add_all(&parse_query_string(&url_decode(body)));
        }
        (host, path, params)
    }

    /// The request when it waits on the network (the online handlers), to be run off the
    /// window's thread; it raises no event.
    pub fn job(&self, url: &str, post: Option<&str>) -> Option<online::Job> {
        let (host, _, params) = Bridge::params(url, post);
        online::job(&host, &params, &self.lobby, || self.host.player_id(), &format!("{}launcher.ini", self.folders.user_config_dir))
    }

    /// `ACSchemeHandler.DoProcess`: the answer and what the window has to do.
    pub fn request(&mut self, url: &str, post: Option<&str>) -> (Answer, Vec<Event>) {
        if let Some(job) = self.job(url, post) {
            return (job(), Vec::new());
        }
        let (host, path, params) = Bridge::params(url, post);
        let mut events = Vec::new();
        let answer = match self.handle(&host, &path, &params, &mut events) {
            Some(Ok(a)) => a,
            Some(Err(t)) => t.answer(),
            None => Answer::plain(false),
        };
        (answer, events)
    }

    fn handle(&mut self, host: &str, path: &str, p: &Params, ev: &mut Vec<Event>) -> Option<Result<Answer, Thrown>> {
        let f = &self.folders;
        Some(match host {
            "exit" => {
                ev.push(Event::Exit);
                Ok(Answer::plain(true))
            }
            "home" => {
                ev.push(Event::Home);
                Ok(Answer::plain(true))
            }
            "listcars" => self.list_cars(ev),
            "listtracks" => self.list_tracks(p, ev),
            "listseasons" => self.list_seasons(),
            "listfiles" => self.list_files(path, p),
            "listspecialevents" => self.list_special_events(),
            "listshowrooms" => self.list_showrooms(),
            "savecombo" | "playsound" => Ok(Answer::plain(true)),
            "start" => {
                appini::format(&appini::get(f, "race"));
                ev.push(Event::Start);
                Ok(Answer::plain(true))
            }
            "getsetting" => self.get_setting(path, p),
            "getsettings" => Ok(self.get_settings(path, p)),
            "setsetting" => self.set_setting(path, p),
            "delsetting" => self.del_setting(path, p),
            "initdata" => Ok(self.init_data(path, p)),
            "getdata" => self.get_data(path, p),
            "setdata" => self.set_data(path, p),
            "deldata" => self.del_data(path, p),
            "stopmusic" => {
                ev.push(Event::StopMusic);
                Ok(Answer::plain(true))
            }
            "startmusic" => {
                ev.push(Event::StartMusic);
                Ok(Answer::plain(true))
            }
            "setvolume" => {
                ev.push(Event::SetVolume);
                Ok(Answer::plain(true))
            }
            "showcontrols" => {
                ev.push(Event::OpenControls);
                Ok(Answer::plain(true))
            }
            "utility" => Ok(self.utility()),
            "copyini" => self.copy_ini(p),
            "generatecss" => Ok(Answer::new(true, "text/css", &self.generated_css)),
            "getiniasjson" => self.get_ini_as_json(path, p),
            "savejsontoini" => self.save_json_to_ini(path, p),
            "savetofile" => self.save_to_file(path, p),
            "getvideomodes" => Ok(self.video_modes()),
            "getcfg" => Ok(self.get_all_cfg()),
            "getskinmeta" => self.get_skin_meta(path),
            "carcount" => Ok(Answer::message(&self.car_count)),
            "eventcount" => Ok(Answer::message(&self.event_count)),
            "formatfile" => {
                let file = path.replace('/', "");
                self.get_file_path(&file).map(|fp| {
                    appini::format(&fp);
                    Answer::message("OK")
                })
            }
            "getguid" | "getid" => Ok(Answer::message(&self.host.player_id())),
            "getresults" => Ok(self.get_results()),
            "getplayername" => Ok(Answer::message(&self.host.player_name())),
            // Steam's UserHasLicenseForApp: the game is installed, so it is owned
            "isowner" => Ok(Answer::message("True")),
            "loadjson" => Ok(self.load_json(path, p)),
            "savejson" => Ok(self.save_json(path, p)),
            // SteamStatisticsReader.exe is not run: no achievements, as when it answers nothing
            "getachievements" => Ok(Answer::json("{}")),
            "getthememodules" => Ok(self.get_theme_modules(p)),
            "getplayersetups" => Ok(self.get_player_setups(path)),
            "showsteamoverlay" => Ok(Answer::message("OK")),
            "listseries" => self.list_series(path),
            "listinis" => self.list_inis(path, p),
            "delfile" => Ok(self.del_file(path)),
            "uimessage" | "loading" | "resizewindow" | "uiready" | "cleanup" => Ok(Answer::message("OK")),
            // left for later (docs/port/launcher.md, Not done): they answer "OK"
            "runtest" | "calibrate" | "showroom" | "requestcode" | "checklicense" | "requestlicense" | "copytoclipboard" | "openforum" | "browseto"
            | "sendbenchmark" | "sendcrash" | "print" | "addtoleaderboard" | "initjslibs" | "js" | "getscripts" | "addreplay" | "listreplays"
            | "delreplay" | "updreplay" | "storereplay" | "getvideopage" | "gethash" | "getcars" => {
                self.not_ported.push(host.to_string());
                Ok(Answer::plain(true))
            }
            _ => return None,
        })
    }

    /// `getFilePath(file)`: the player's cfg file, which must be there.
    fn get_file_path(&self, file: &str) -> Result<String, Thrown> {
        let text = appini::get(&self.folders, file);
        if file.is_empty() {
            return Err(Thrown::ArgumentOutOfRange);
        }
        if !net::file_exists(&text) {
            return Err(Thrown::FileNotFound(text));
        }
        Ok(text)
    }

    fn get_setting(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let text = path.replace('/', "");
        let file = self.get_file_path(&text)?;
        let value = p.at(0)?.ok_or(Thrown::NullReference)?;
        let parts = split_once_slash(&value);
        let (section, key) = if parts.len() > 1 { (parts[0].to_string(), parts[1].to_string()) } else { (text, value.clone()) };
        Ok(Answer::message(&IniFile::new(file).read(Some(&section), Some(&key))))
    }

    fn get_settings(&self, path: &str, p: &Params) -> Answer {
        let text = path.replace('/', "");
        let mut out = String::new();
        if net::to_lower(&text) == "primaryresolution" {
            let (w, h) = self.host.primary_resolution();
            out = format!("{},{}", jss::double_r(w), jss::double_r(h));
        } else {
            let ini = IniFile::new(appini::get(&self.folders, &text));
            for i in 0..p.len() {
                let key = p.key(i).unwrap();
                out.push_str(&ini.read(Some(&text), key.as_deref()));
                if i + 1 < p.len() {
                    out.push(',');
                }
            }
        }
        Answer::new(true, "text/html", &out)
    }

    fn set_setting(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let text = path.replace('/', "");
        let file = self.get_file_path(&text)?;
        let key = p.key(0)?;
        let value = p.get_opt(key.as_deref());
        let key = key.ok_or(Thrown::NullReference)?;
        let parts = split_once_slash(&key);
        let (section, key) = if parts.len() > 1 { (parts[0].to_string(), parts[1].to_string()) } else { (text, key.clone()) };
        let value = value.map(|v| url_decode(&v));
        IniFile::new(file).write(Some(&net::to_upper(&section)), Some(&net::to_upper(&key)), value.as_deref())?;
        Ok(Answer::plain(true))
    }

    fn delete_sections(ini: &IniFile, prefix: &str) -> Result<(), Thrown> {
        for item in ini.categories()? {
            if !item.is_empty() && net::to_lower(&item).starts_with(&net::to_lower(prefix)) {
                ini.write(Some(&item), None, None)?;
            }
        }
        Ok(())
    }

    fn del_setting(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let file = self.get_file_path(&path.replace('/', ""))?;
        let ini = IniFile::new(file);
        match p.get("prefix").filter(|v| !v.is_empty()) {
            Some(prefix) => Bridge::delete_sections(&ini, &prefix)?,
            None => {
                let first = p.at(0)?.ok_or(Thrown::NullReference)?;
                let parts = split_once_slash(&first);
                ini.write(Some(parts[0]), parts.get(1).copied(), None)?;
            }
        }
        Ok(Answer::plain(true))
    }

    fn data_path(&self, path: &str, p: &Params) -> String {
        if p.get("folder").as_deref() == Some("out") {
            format!("{}{path}.ini", self.folders.user_out_dir)
        } else {
            format!("{}{path}.ini", self.folders.user_launcher_data_file_store_dir)
        }
    }

    fn init_data(&self, path: &str, p: &Params) -> Answer {
        let file = self.data_path(path, p);
        let mut msg = "OK";
        if !net::file_exists(&file) && net::write_all_text_bom(&file, "").is_err() {
            msg = "ERROR";
        }
        Answer::message(msg)
    }

    fn get_data(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let text = path.replace('/', "");
        let ini = IniFile::new(self.data_path(path, p));
        let value = p.at(0)?.ok_or(Thrown::NullReference)?;
        let parts = split_once_slash(&value);
        let (section, key) = if parts.len() > 1 { (parts[0].to_string(), parts[1].to_string()) } else { (text, value.clone()) };
        Ok(Answer::message(&ini.read(Some(&section), Some(&key))))
    }

    fn set_data(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let text = path.replace('/', "");
        let ini = IniFile::new(self.data_path(path, p));
        let key = p.key(0)?;
        let value = p.get_opt(key.as_deref());
        let key = key.ok_or(Thrown::NullReference)?;
        let parts = split_once_slash(&key);
        let (section, key) = if parts.len() > 1 { (parts[0].to_string(), parts[1].to_string()) } else { (text, key.clone()) };
        let value = value.map(|v| url_decode(&v));
        ini.write(Some(&net::to_upper(&section)), Some(&net::to_upper(&key)), value.as_deref())?;
        Ok(Answer::plain(true))
    }

    fn del_data(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let ini = IniFile::new(self.data_path(path, p));
        match p.get("prefix").filter(|v| !v.is_empty()) {
            Some(prefix) => Bridge::delete_sections(&ini, &prefix)?,
            None => {
                let first = p.key(0)?.ok_or(Thrown::NullReference)?;
                let parts = split_once_slash(&first);
                ini.write(Some(parts[0]), parts.get(1).copied(), None)?;
            }
        }
        Ok(Answer::plain(true))
    }

    fn copy_ini(&self, p: &Params) -> Result<Answer, Thrown> {
        let f = &self.folders;
        let (src, dst) = if p.get("location").as_deref() == Some("userdata") {
            (&f.user_data_path, &f.user_data_path)
        } else {
            (&f.user_template_dir, &f.user_config_dir)
        };
        let source = format!("{src}{}.ini", p.get("source").unwrap_or_default());
        let target = format!("{dst}{}.ini", p.get("target").unwrap_or_default());
        if net::file_exists(&source) {
            std::fs::copy(&source, &target).map_err(|e| io_thrown(&target, &e))?;
        }
        Ok(Answer::message(if net::file_exists(&source) { "True" } else { "False" }))
    }

    fn get_ini_as_json(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let f = &self.folders;
        let mut text = path.replace('/', "\\");
        if !text.ends_with(".ini") {
            text.push_str(".ini");
        }
        let mut file = format!("{}{text}", if p.get("root").as_deref() == Some("true") { &f.app_path } else { &f.user_data_path });
        if p.get("filestore").as_deref() == Some("true") {
            file = format!("{}{text}", f.user_launcher_data_file_store_dir);
        }
        let asarray = p.get("asarray").as_deref() == Some("true");
        let lowercase = p.get("lowercase").as_deref() == Some("true");
        let ignore: Vec<String> = p.get("ignoresections").unwrap_or_default().split(',').map(str::to_string).collect();
        let s = if !net::file_exists(&file) {
            if asarray { "{\"aaData\":[]}".to_string() } else { "File Not Found".to_string() }
        } else {
            appini::get_as_json(&file, asarray, &ignore, !lowercase)?
        };
        Ok(Answer::json(&s))
    }

    fn get_all_cfg(&self) -> Answer {
        let dir = &self.folders.user_config_dir;
        let files = net::files_with(dir, ".ini").unwrap_or_default();
        let mut text = String::from("{");
        for (i, file) in files.iter().enumerate() {
            let name = net::to_lower(&file.replace(dir.as_str(), "").replace(".ini", ""));
            // an exception ends the list where it is
            let Ok(json) = appini::get_as_json(file, false, &[], true) else { break };
            if json.len() > 1 {
                text.push_str(&format!("\"{name}\" : {json}"));
            }
            if i + 1 < files.len() {
                text.push(',');
            }
        }
        text.push('}');
        Answer::json(&text)
    }

    fn save_to_file(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let text = path.replace('/', "\\");
        let mut location = p.get("location").unwrap_or_default();
        if location != "appdata" && location != "userdata" {
            location = "userdata".into();
        }
        let data = p.get("data");
        // "appdata" is the game's own themes folder, which rustyAC never writes: the file
        // goes to the player's themes folder instead
        let file = format!("{}{text}", self.folders.user_themes_dir);
        let _ = location;
        if let Some(d) = &data {
            net::write_all_text_bom(&file, d).map_err(|e| io_thrown(&file, &e))?;
        }
        // Encoding.UTF8.GetBytes(null) throws
        let data = data.ok_or(Thrown::Other("Value cannot be null.\r\nParameter name: s".into()))?;
        Ok(Answer::json(&data))
    }

    fn save_json_to_ini(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let ini = IniFile::new(format!("{}{}", self.folders.user_data_path, path.replace('/', "\\")));
        for i in 0..p.len() {
            let name = p.key(i)?.ok_or(Thrown::NullReference)?;
            let units: Vec<u16> = name.encode_utf16().collect();
            let num = units.iter().position(|&u| u == b'[' as u16).map_or(-1, |n| n as isize);
            let section = if num != -1 { String::from_utf16_lossy(&units[..num as usize]) } else { name.clone() };
            // Substring(num + 1, Length - num - 2)
            let start = (num + 1) as usize;
            let len = units.len() as isize - num - 2;
            if len < 0 {
                return Err(Thrown::ArgumentOutOfRange);
            }
            let key = String::from_utf16_lossy(&units[start..start + len as usize]);
            let value = p.at(i)?;
            ini.write(Some(&section), Some(&key), value.as_deref())?;
        }
        Ok(Answer::json(&format!("{{ \"success\" : \"true\", \"message\" : \"Saved as {path}\" }}")))
    }

    fn get_skin_meta(&self, path: &str) -> Result<Answer, Thrown> {
        let parts: Vec<&str> = path.get(1..).unwrap_or("").split('/').collect();
        if parts.len() < 2 {
            return Err(Thrown::Other("Index was outside the bounds of the array.".into()));
        }
        let file = format!("{}cars\\{}\\skins\\{}\\ui_skin.json", self.folders.default_content_dir, parts[0], parts[1]);
        let text = net::read_all_text(&file).map_err(|e| io_thrown(&file, &e))?;
        Ok(Answer::json(&text))
    }

    fn list_files(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let f = &self.folders;
        let ext = p.get("extension").unwrap_or_default();
        let base = if p.get("root").is_some() { &f.app_path } else { &f.user_data_path };
        let dir = format!("{base}{}\\", path.replace('/', "\\"));
        let files = match net::files_with(&dir, &format!(".{ext}")) {
            Ok(files) => files,
            Err(_) => return Ok(Answer::message(&format!("ERROR, Could not find a part of the path '{}'.", net::full_path(&dir)))),
        };
        let list: Vec<Value> = files
            .iter()
            .map(|file| {
                let name = file.replace(dir.as_str(), "");
                Value::Array(vec![Value::Str(name.replace(&format!(".{ext}"), "")), Value::Str(name)])
            })
            .collect();
        Ok(Answer::json(&jss::serialize(&Value::Array(list))))
    }

    fn utility(&self) -> Answer {
        let mut last = String::new();
        let mut text = String::from("{");
        for (w, h) in self.host.display_modes() {
            let mode = format!("{w} x {h}");
            if last != mode {
                last = mode;
                text.push_str(&format!("\"{w}x{h}\":\"{w},{h}\","));
            }
        }
        text.pop();
        text.push('}');
        Answer::json(&text)
    }

    fn video_modes(&self) -> Answer {
        let Some(modes) = self.host.video_modes().filter(|m| !m.is_empty()) else { return Answer::json("[]") };
        let list: Vec<Value> = modes
            .iter()
            .enumerate()
            .map(|(i, &(w, h, r))| {
                // Math.Round(float -> double, 2).ToString()
                let r = round_even(r as f64 * 100.0) / 100.0;
                Value::Array(
                    [(i + 1).to_string(), w.to_string(), h.to_string(), crate::jss::double_r(r), "0".into(), "0".into(), "UNSPECIFIED".into(), "PROGRESSIVE".into()]
                        .into_iter()
                        .map(Value::Str)
                        .collect(),
                )
            })
            .collect();
        Answer::json(&jss::serialize(&Value::Array(list)))
    }

    fn get_results(&self) -> Answer {
        let race_out = format!("{}race_out.json", self.folders.user_out_dir);
        let warnings = format!("{}warnings.json", self.folders.user_out_dir);
        let exists = net::file_exists(&race_out);
        let [y, mo, d, h, mi, s, ms] = self.host.local_time(exists.then_some(race_out.as_str()));
        let days = days_from_civil(y, mo, d);
        let total = ((days * 24 + h) * 60 + mi) * 60_000 + s * 1000 + ms;
        let date = format!("{d:02} {} {y:04}|{h:02}:{mi:02}|{total}", self.host.month_abbrev(mo as u32));
        let mut text = format!("{{ \"date\": \"{date}\", \"resultData\" : ");
        text.push_str(&if exists { net::read_all_text(&race_out).unwrap_or_default() } else { "{}".into() });
        text.push_str(", \"warnings\":");
        text.push_str(&if net::file_exists(&warnings) { net::read_all_text(&warnings).unwrap_or_default() } else { "{}".into() });
        text.push('}');
        Answer::json(&text)
    }

    fn load_json(&self, path: &str, p: &Params) -> Answer {
        let path = path.replace('/', "\\");
        let root = p.get("root").filter(|r| !r.trim().is_empty()).is_some_and(|r| r == "true");
        let file = format!("{}{}", if root { &self.folders.default_themes_dir } else { &self.folders.user_data_path }, url_decode(&path));
        let data = if net::file_exists(&file) { net::read_all_text(&file).unwrap_or_else(|_| "{}".into()) } else { "{}".into() };
        Answer::json(&data)
    }

    fn del_file(&self, path: &str) -> Answer {
        let file = format!("{}{}", self.folders.user_data_path, url_decode(&path.replace('/', "\\")));
        if net::file_exists(&file) && std::fs::remove_file(&file).is_ok() {
            return Answer::message("OK");
        }
        Answer::message("ERROR")
    }

    fn save_json(&self, path: &str, p: &Params) -> Answer {
        match self.try_save_json(path, p) {
            Ok(()) => Answer::message("OK"),
            Err(m) => Answer::message(&format!("ERROR, {m}")),
        }
    }

    fn try_save_json(&self, path: &str, p: &Params) -> Result<(), String> {
        let path = path.trim_end_matches('/').replace('/', "\\");
        let reserved = ["format", "clear", "ignore"];
        let some = |name: &str| p.get(name).filter(|v| !v.trim().is_empty());
        let format = some("format").unwrap_or_else(|| "json".into());
        let clear = some("clear").is_some_and(|v| v == "true");
        let root = some("root").is_some_and(|v| v == "true");
        let ignore: Vec<String> = some("ignore").map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default();
        let thrown = |t: Thrown| match t {
            Thrown::Other(m) => m,
            Thrown::FileNotFound(f) => format!("Could not find file '{f}'."),
            Thrown::NullReference => "Object reference not set to an instance of an object.".into(),
            Thrown::ArgumentOutOfRange => "Index was out of range. Must be non-negative and less than the size of the collection.\r\nParameter name: index".into(),
        };
        if format == "ini" {
            let file = format!("{}{path}.ini", if root { &self.folders.user_data_path } else { &self.folders.user_config_dir });
            if clear || !net::file_exists(&file) {
                net::write_all_text_bom(&file, "").map_err(|e| thrown(io_thrown(&file, &e)))?;
            }
            let ini = IniFile::new(file.clone());
            let mut last = String::new();
            for name in p.keys() {
                let Some(name) = name else { return Err(thrown(Thrown::NullReference)) };
                if reserved.contains(&name) {
                    continue;
                }
                let trimmed = name.trim_end_matches([' ', ']']);
                let parts: Vec<&str> = trimmed.split('[').collect();
                let section = parts[0];
                let key = parts.get(1).copied().unwrap_or("");
                if !ignore.iter().any(|i| i == section) {
                    if last != section && clear {
                        last = section.to_string();
                        let mut fh = std::fs::OpenOptions::new().append(true).open(&file).map_err(|e| thrown(io_thrown(&file, &e)))?;
                        std::io::Write::write_all(&mut fh, b"\n").map_err(|e| e.to_string())?;
                    }
                    ini.write(Some(section), Some(key), p.get(name).as_deref()).map_err(thrown)?;
                }
            }
            if !clear {
                appini::format(&file);
            }
        } else {
            let file = format!("{}{}", self.folders.user_data_path, url_decode(&path));
            let name = net::file_name(&file);
            let folder = file.replace(&format!("\\{name}"), "");
            if !net::dir_exists(&folder) {
                std::fs::create_dir_all(&folder).map_err(|e| thrown(io_thrown(&folder, &e)))?;
            }
            let data = p.at(0).map_err(thrown)?.ok_or_else(|| thrown(Thrown::Other("String reference not set to an instance of a String.\r\nParameter name: s".into())))?;
            net::write_all_text_bom(&file, &data).map_err(|e| thrown(io_thrown(&file, &e)))?;
        }
        Ok(())
    }

    fn get_theme_modules(&self, p: &Params) -> Answer {
        let f = &self.folders;
        let modules = format!("{}\\modules", p.get("theme").unwrap_or_default());
        let default_dir = format!("{}{modules}\\", f.default_themes_dir);
        let user_dir = format!("{}{modules}\\", f.user_themes_dir);
        let mut all = net::list(&default_dir, true).unwrap_or_default();
        all.extend(net::list(&user_dir, true).unwrap_or_default());
        let apps = net::list(&f.default_apps_dir, true).unwrap_or_default();
        let mut dict = Value::obj();
        for dir in &all {
            let ini_path = format!("{dir}\\module.ini");
            if !dir.starts_with('.') && net::file_exists(&ini_path) {
                let ini = IniFile::new(ini_path);
                let name = net::file_name(dir);
                let module = Value::Object(vec![
                    ("classname".into(), Value::str(ini.read_or("MODULE", "class", name))),
                    ("name".into(), Value::str(ini.read_or("MODULE", "name", name))),
                    ("description".into(), Value::str(ini.read(Some("MODULE"), Some("description")))),
                    ("restart".into(), Value::Bool(ini.read_or("MODULE", "restart", "1") == "1")),
                    ("userdocs".into(), Value::Bool(!dir.contains(f.default_themes_dir.as_str()))),
                    ("path".into(), Value::str(net::uri_path(dir))),
                    ("pythonapp".into(), Value::Bool(false)),
                    ("steamworkshop".into(), Value::Bool(false)),
                ]);
                dict.set(name, module);
            }
        }
        for dir in &apps {
            let name = net::file_name(dir);
            if name == "system" {
                continue;
            }
            let module = Value::Object(vec![
                ("classname".into(), Value::Null),
                ("name".into(), Value::str(name)),
                ("description".into(), Value::str("in-game app")),
                ("restart".into(), Value::Bool(false)),
                ("userdocs".into(), Value::Bool(!dir.contains(f.default_themes_dir.as_str()))),
                ("path".into(), Value::str(net::uri_path(dir))),
                ("pythonapp".into(), Value::Bool(true)),
                ("steamworkshop".into(), Value::Bool(false)),
            ]);
            dict.set(&format!("app/{name}"), module);
        }
        Answer::json(&jss::serialize(&dict))
    }

    fn get_player_setups(&self, path: &str) -> Answer {
        let dir = format!("{}{path}\\", self.folders.user_setup_dir);
        let mut list = Vec::new();
        if net::dir_exists(&dir) {
            for file in net::files_with(&dir, ".ini").unwrap_or_default() {
                list.push(Value::Str(file.replace(dir.as_str(), "").replace(".ini", "")));
            }
        }
        Answer::json(&jss::serialize(&Value::Array(list)))
    }

    fn list_showrooms(&self) -> Result<Answer, Thrown> {
        let f = &self.folders;
        let _ = create_if_non_existant(&format!("{}showroom", f.user_content_dir));
        let default = format!("{}showroom", f.default_content_dir);
        let mut folders = Vec::new();
        if net::dir_exists(&default) {
            folders = dirs(&default)?;
            folders.extend(dirs(&format!("{}showroom", f.user_content_dir))?);
        }
        let mut dict = Value::obj();
        for item in &folders {
            let name = net::file_name(item);
            let file = format!("{item}\\ui\\\\ui_showroom.json");
            let fallback = || {
                Value::Object(vec![
                    ("id".into(), Value::str(name)),
                    ("name".into(), Value::str(name)),
                    ("path".into(), Value::str(net::uri_path(item))),
                    ("noJSON".into(), Value::Bool(true)),
                ])
            };
            let entry = if net::file_exists(&file) {
                match net::read_all_text(&file).ok().map(|t| jss::deserialize_dictionary(&t)) {
                    Some(Ok(Some(mut d))) if !d.contains("id") && !d.contains("path") => {
                        d.add("id", Value::str(net::to_lower(name)));
                        d.add("path", Value::str(net::uri_path(item)));
                        d
                    }
                    Some(Ok(None)) => return Err(Thrown::NullReference),
                    _ => fallback(),
                }
            } else {
                fallback()
            };
            if !dict.add(&net::to_lower(name), entry) {
                return Err(Thrown::Other("An item with the same key has already been added.".into()));
            }
        }
        Ok(Answer::json(&jss::serialize(&dict)))
    }

    fn list_cars(&mut self, ev: &mut Vec<Event>) -> Result<Answer, Thrown> {
        let f = self.folders.clone();
        let _ = create_if_non_existant(&format!("{}cars", f.user_content_dir));
        let cars_dir = format!("{}cars", f.default_content_dir);
        let folders = dirs(&cars_dir)?;
        let mut dict = Value::obj();
        let mut error = String::new();
        let count = folders.len();
        for (i, text) in folders.iter().enumerate() {
            let file_name = net::file_name(text);
            let id = net::to_lower(file_name);
            self.car_count = format!("{}%", round_even(100.0 * i as f64 / count as f64) as i64);
            let ui = format!("{text}\\ui\\");
            let mut path = format!("{text}\\ui\\ui_car.json");
            let mut dlc = false;
            if !net::file_exists(&path) {
                dlc = true;
                path = format!("{text}\\ui\\dlc_ui_car.json");
            }
            let mut d = Value::obj();
            d.add("dlc", Value::Bool(dlc));
            let no_data = |d: &mut Value| {
                d.set("id", Value::str(id.clone()));
                d.set("name", Value::str(file_name));
                d.set("description", Value::str("No car data available."));
                d.set("noJSON", Value::Bool(true));
                d.set("brand", Value::str("unknown"));
                d.set("tags", Value::Array(Vec::new()));
                d.set("class", Value::str("unknown"));
            };
            if net::file_exists(&path) {
                match net::read_all_text(&path).map_err(|_| ()).and_then(|t| jss::deserialize_dictionary(&t)) {
                    Ok(Some(parsed)) => {
                        d = parsed;
                        d.set("path", Value::str(net::uri_path(text)));
                        d.set("id", Value::str(id.clone()));
                        let badge = format!("{ui}badge.png");
                        d.set("badge", Value::str(if net::file_exists(&badge) { net::uri_path(&badge) } else { "false".into() }));
                        if !d.contains("brand") {
                            d.set("brand", Value::str("unknown"));
                        }
                        if !d.contains("tags") {
                            d.set("tags", Value::Array(Vec::new()));
                        }
                        if !d.contains("class") {
                            d.set("class", Value::str("unknown"));
                        }
                        if dlc {
                            let preview = format!("{ui}dlc_preview.png");
                            d.set("preview", Value::str(if net::file_exists(&preview) { net::uri_path(&preview) } else { "false".into() }));
                        }
                    }
                    // `null`: the dictionary is null and the catch block throws again
                    Ok(None) => return Err(Thrown::NullReference),
                    Err(()) => {
                        no_data(&mut d);
                        error.push_str(&format!("{file_name}, "));
                    }
                }
                d.set("dlc", Value::Bool(dlc));
            } else {
                no_data(&mut d);
            }
            let skins_dir = format!("{text}\\skins");
            let mut skins = Vec::new();
            let mut meta = Value::obj();
            if net::dir_exists(&skins_dir) {
                for skin in dirs(&skins_dir)?.iter().map(|s| net::to_lower(s)) {
                    let name = net::file_name(&skin).to_string();
                    self.generated_css.push_str(&format!(".skin-{id}-{name} {{\n\tbackground-image: url(\"{}/preview.jpg\") !important;\n}}\n\n", net::uri_path(&skin)));
                    skins.push(Value::Str(name.clone()));
                    let data = Value::Object(["skinName", "driverName", "country", "number", "team"].iter().map(|k| (k.to_string(), Value::Null)).collect());
                    if !meta.add(&name, data) {
                        error.push_str(&format!("{file_name}/{name}, "));
                    }
                }
            }
            if !d.add("skins", Value::Array(skins)) || !d.add("skinmeta", meta) {
                return Err(Thrown::Other("An item with the same key has already been added.".into()));
            }
            dict.add(&id, d);
        }
        let text = jss::serialize(&dict);
        if !error.is_empty() {
            ev.push(Event::ContentErrors(error));
        }
        let cache = format!("{}\\cache_car.json", f.user_launcher_data_dir);
        net::write_all_text(&cache, &text).map_err(|e| io_thrown(&cache, &e))?;
        Ok(Answer::json(&text))
    }

    fn list_tracks(&mut self, p: &Params, ev: &mut Vec<Event>) -> Result<Answer, Thrown> {
        let f = self.folders.clone();
        let asarray = p.get("asarray").is_some_and(|v| net::to_lower(v.trim_matches(|c: char| c.is_whitespace() || c == '\0')) == "true");
        let mut error = String::new();
        let mut dict = Value::obj();
        let mut list = Vec::new();
        let _ = create_if_non_existant(&format!("{}tracks", f.user_content_dir));
        for text in dirs(&format!("{}tracks", f.default_content_dir))? {
            let file_name = net::file_name(&text).to_string();
            let ui = format!("{text}\\ui\\");
            if !net::dir_exists(&ui) {
                continue;
            }
            let layouts = dirs(&ui)?;
            for j in 0..=layouts.len() {
                let (folder, layout) = if j < layouts.len() { (format!("{}\\", layouts[j]), net::file_name(&layouts[j]).to_string()) } else { (ui.clone(), String::new()) };
                let mut path = format!("{folder}ui_track.json");
                let mut dlc = false;
                if !net::file_exists(&path) {
                    dlc = true;
                    path = format!("{folder}dlc_ui_track.json");
                }
                if !net::file_exists(&path) {
                    continue;
                }
                let id = if layout.is_empty() { file_name.clone() } else { format!("{file_name}-{}", net::to_lower(&layout)) };
                let input = net::read_all_text(&path).unwrap_or_default();
                let mut d = match jss::deserialize_dictionary(&input) {
                    Ok(Some(d)) => d,
                    Ok(None) => return Err(Thrown::NullReference),
                    Err(()) => {
                        let mut d = Value::obj();
                        d.set("name", Value::str(file_name.clone()));
                        d.set("noJSON", Value::Bool(true));
                        d.set("description", Value::str("Invalid JSON Data."));
                        error.push_str(&format!("{file_name}, "));
                        d
                    }
                };
                d.set("id", Value::str(id.clone()));
                d.set("track", Value::str(file_name.clone()));
                d.set("configuration", Value::str(layout.clone()));
                d.set("dlc", Value::Bool(dlc));
                let file_or_false = |name: &str| {
                    let p = format!("{folder}{name}");
                    Value::str(if net::file_exists(&p) { net::uri_path(&p) } else { "false".into() })
                };
                d.set("preview", file_or_false(if dlc { "dlc_preview.png" } else { "preview.png" }));
                d.set("outline", file_or_false("outline.png"));
                d.set("video", file_or_false("video.ogv"));
                if !dict.add(&id, d) {
                    error.push_str(&format!("{id}/{layout}, "));
                }
            }
            if asarray {
                list.push(std::mem::replace(&mut dict, Value::obj()));
            }
        }
        let text = if asarray { jss::serialize(&Value::Array(list)) } else { jss::serialize(&dict) };
        if !error.is_empty() {
            ev.push(Event::ContentErrors(error));
        }
        let cache = format!("{}\\cache_track.json", f.user_launcher_data_dir);
        net::write_all_text(&cache, &text).map_err(|e| io_thrown(&cache, &e))?;
        Ok(Answer::json(&text))
    }

    fn list_special_events(&mut self) -> Result<Answer, Thrown> {
        let f = self.folders.clone();
        let base = format!("{}specialevents", f.default_content_dir);
        let mut folders: Vec<(String, std::time::SystemTime)> = dirs(&base)?
            .into_iter()
            .map(|d| {
                let t = std::fs::metadata(&d).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                (d, t)
            })
            .collect();
        // OrderByDescending(LastWriteTime), a stable sort
        folders.sort_by_key(|f| std::cmp::Reverse(f.1));
        let mut text = String::from("{");
        let n = folders.len();
        for (num, (full, _)) in folders.iter().enumerate() {
            let name = net::to_lower(net::file_name(full));
            if !text.ends_with('{') {
                text.push(',');
            }
            let json = appini::get_as_json(&format!("{full}\\event.ini"), false, &[], true)?;
            text.push_str(&format!("\"{name}\" : {{ \"path\" : \"{}\", \"info\":  {json} }} ", net::uri_path(full)));
            self.event_count = format!("{}", round_even(num as f64 / n as f64 * 100.0) as i64);
        }
        text.push('}');
        let cache = format!("{}\\cache_events.json", f.user_launcher_data_dir);
        net::write_all_text(&cache, &text).map_err(|e| io_thrown(&cache, &e))?;
        Ok(Answer::json(&text))
    }

    fn career_dirs(&self, pattern: &str, prefix: &str) -> Result<Vec<String>, Thrown> {
        let base = format!("{}career", self.folders.default_content_dir);
        let mut list: Vec<String> = dirs(&base)?.into_iter().filter(|d| net::to_lower(net::file_name(d)).starts_with(pattern)).collect();
        let prefix = format!("{base}\\{prefix}");
        sort::introsort(&mut list, &|a: &String, b: &String| sort::alphanumeric_key(a, &prefix).cmp(&sort::alphanumeric_key(b, &prefix)));
        Ok(list)
    }

    fn list_seasons(&self) -> Result<Answer, Thrown> {
        let base = format!("{}career\\", self.folders.default_content_dir);
        let mut text = String::from("{");
        for dir in self.career_dirs("season", "season")? {
            let name = dir.replace(&base, "");
            let season = format!("{dir}\\season.ini");
            let opponents = format!("{dir}\\opponents.ini");
            if net::file_exists(&season) && net::file_exists(&opponents) {
                if !text.ends_with('{') {
                    text.push(',');
                }
                let a = appini::get_as_json(&season, false, &[], true)?;
                let b = appini::get_as_json(&opponents, false, &[], true)?;
                text.push_str(&format!("\"{name}\" : {{ \"path\" : \"{}\", \"info\":  {a}, \"opponents\" : {b} }} ", net::uri_path(&dir)));
            }
        }
        text.push('}');
        Ok(Answer::json(&text))
    }

    fn list_series(&self, path: &str) -> Result<Answer, Thrown> {
        let champs = path == "/champs";
        let f = &self.folders;
        let base = format!("{}career", f.default_content_dir);
        let series = self.career_dirs("series", "series")?;
        let b = |v: bool| if v { "true" } else { "false" };
        let mut text = String::from("{");
        if !champs {
            text.push_str(&format!("\"videoStart\":{},", b(net::file_exists(&format!("{base}\\start.ogv")))));
            text.push_str(&format!("\"path\" : \"{}\" ", net::uri_path(&base)));
            for dir in &series {
                let name = dir.replace(&format!("{base}\\"), "");
                let ini = format!("{dir}\\series.ini");
                let opponents = format!("{dir}\\opponents.ini");
                if !net::file_exists(&ini) {
                    continue;
                }
                if !text.ends_with('{') {
                    text.push(',');
                }
                let info = appini::get_as_json(&ini, false, &[], false)?;
                let opp = if net::file_exists(&opponents) { appini::get_as_json(&opponents, false, &[], false)? } else { "null".into() };
                let mut events: Vec<String> = dirs(dir)?.into_iter().filter(|d| net::to_lower(net::file_name(d)).starts_with("event")).collect();
                let prefix = format!("{dir}\\event");
                sort::introsort(&mut events, &|a: &String, c: &String| sort::alphanumeric_key(a, &prefix).cmp(&sort::alphanumeric_key(c, &prefix)));
                let mut list = Vec::new();
                for e in &events {
                    let ei = format!("{e}\\event.ini");
                    if net::file_exists(&ei) {
                        list.push(appini::get_as_json(&ei, false, &[], false)?);
                    }
                }
                text.push_str(&format!(
                    "\"{name}\" : {{\"career\": true, \"videoStart\" : {}, \"videoSuccess\" : {}, \"videoFailure\" : {}, \"path\" : \"{}\", \"info\":  {info}, \"opponents\" : {opp}, \"events\" : [ {} ] }} ",
                    b(net::file_exists(&format!("{dir}\\start.ogv"))),
                    b(net::file_exists(&format!("{dir}\\success.ogv"))),
                    b(net::file_exists(&format!("{dir}\\failure.ogv"))),
                    net::uri_path(dir),
                    list.join(",")
                ));
            }
        }
        text.push('}');
        let cache = format!("{}\\cache_career.json", f.user_launcher_data_dir);
        net::write_all_text(&cache, &text).map_err(|e| io_thrown(&cache, &e))?;
        Ok(Answer::json(&text))
    }

    fn list_inis(&self, path: &str, p: &Params) -> Result<Answer, Thrown> {
        let base = format!("{}{path}\\", self.folders.default_content_dir);
        if !net::dir_exists(&base) {
            return Ok(Answer::json("{}"));
        }
        let mut list = dirs(&base)?;
        let filter = p.get("filter").unwrap_or_default();
        if p.get("alphanumeric").as_deref() != Some("false") {
            sort::introsort(&mut list, &|a: &String, b: &String| sort::alphanumeric_key(a, &base).cmp(&sort::alphanumeric_key(b, &base)));
        }
        let mut text = String::from("{");
        let n = list.len();
        for (i, dir) in list.iter().enumerate() {
            let name = dir.replace(base.as_str(), "");
            let files = files_with(dir, &format!("{filter}.ini"))?;
            if !files.is_empty() {
                text.push_str(&format!("\"{name}\": "));
                for file in &files {
                    text.push_str(&appini::get_as_json(file, false, &[], false)?);
                }
                if i + 1 != n {
                    text.push(',');
                }
            }
        }
        text.push('}');
        Ok(Answer::json(&text))
    }
}

/// `Math.Round(x)`: halves to even.
pub fn round_even(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        r
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
