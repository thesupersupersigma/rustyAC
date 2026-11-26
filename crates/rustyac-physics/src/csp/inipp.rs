// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2a: CSP's INIpp (`utils::ini_file`, dwrite.dll 0.3.2-preview2 build 4305) with Lua off,
//! the reader CSP's car and track config loaders run on `ext_config.ini` and the files it includes
//! (docs/csp/lua_host.md, "C6a part 2"). [`flatten`] gives the text `[__DEBUG] DUMP_FLATTEN_INI`
//! writes. Names in comments are the dll's functions (imagebase 0x180000000).
//!
//! Strings are bytes as in CSP (std::string, no encoding).

use std::path::{Path, PathBuf};

type B = Vec<u8>;
type Vars = Vec<(B, Vec<B>)>;

/// `$"..."` (a Lua expression) as the splitter marks it (unk_182373B3C).
const LUA_MARK: &[u8] = &[0x5b, 0x01, 0x02, 0x04, 0x01];
/// Its end (qword_1826C2488, set at run time; CSP's ends in these bytes, only uniqueness matters).
const LUA_END: &[u8] = &[0x01, 0x02, 0x04, 0x5d];

#[cfg(feature = "lua")]
use super::inipp_lua::Lua;
/// Without feature `lua` there is no Lua: CSP with Lua off.
#[cfg(not(feature = "lua"))]
enum Lua {}
#[cfg(not(feature = "lua"))]
impl Lua {
    fn eval(&self, _: &[u8], _: &[u8], _: &[u8], _: &mut Vec<B>) -> bool {
        match *self {}
    }
    fn function(&self, _: &[u8], _: &[B], _: &[u8]) {
        match *self {}
    }
    fn use_file(&self, _: &Path) {
        match *self {}
    }
}
/// Our stand-in for CSP's runtime `_...` key marker (qword_1826C2428): only its being constant matters.
const KEY_MARK: &[u8] = &[0x01];

#[derive(Clone, Default)]
struct Section {
    name: B,
    keys: Vars,
    /// set: the keys go to this template (or mixin) instead (section +64)
    tpl: Option<B>,
    /// templates applied when the section ends (section +80)
    tpls: Vec<B>,
    /// keys each template set here, which it may set again (section +112)
    rec: Vec<(B, Vec<B>)>,
}

impl Section {
    fn named(name: &[u8]) -> Self {
        Section { name: name.to_vec(), ..Default::default() }
    }
    fn get(&self, k: &[u8]) -> Option<&Vec<B>> {
        self.keys.iter().find(|(n, _)| n == k).map(|(_, v)| v)
    }
    fn set(&mut self, k: B, v: Vec<B>) {
        set_var(&mut self.keys, k, v);
    }
    fn remove(&mut self, k: &[u8]) {
        self.keys.retain(|(n, _)| n != k);
    }
}

fn set_var(list: &mut Vars, k: B, v: Vec<B>) {
    match list.iter_mut().find(|(n, _)| *n == k) {
        Some(e) => e.1 = v,
        None => list.push((k, v)),
    }
}

#[derive(Clone, Default)]
struct Template {
    keys: Vars,
    parents: Vec<B>,
    early: bool,
    /// the variables where it was first named (template +48)
    // shortcut: a copy, CSP keeps the live scope (later DEFAULTS of that file would show), upgrade if a config needs it
    scope: Vec<Vars>,
}

/// A `${...}` or `$name` reference (sub_18148F100's result).
#[derive(Default)]
struct Ref {
    name: B,
    default: B,
    start: i64,
    end: i64,
    simple: bool,
    required: bool,
    modi: u8,
}

#[derive(Default)]
struct Parser {
    /// finalized sections in order (parser +24)
    done: Vec<Section>,
    templates: Vec<(B, Template)>,
    mixins: Vec<(B, Template)>,
    /// the `_...` key counter (parser +256)
    counter: u64,
    file: PathBuf,
    /// include search folders found so far (parser +160)
    dirs: Vec<PathBuf>,
    /// (file, variables) pairs already read (sub_18149A620)
    seen: Vec<(PathBuf, Vars)>,
    /// variables the `[INCLUDE]` sections around the current file give: no `[DEFAULTS]` overrides them
    given: Vec<B>,
    /// the parser's Lua state (parser +192..+248 flags, sub_181489730); `None`: Lua off
    lua: Option<Lua>,
    /// `read()`'s data provider (the loader's; none: only csp/version)
    #[cfg(feature = "lua")]
    read: Option<Reader>,
    /// where `PREFIX_...` section numbers start (sub_18149B1C0's argument)
    base: i64,
}

/// A loader's answer to Lua's `read(path)`, `None` for none.
pub type Reader = Box<dyn Fn(&[u8]) -> Option<B>>;

/// Resolved sections: name, then keys with their values.
pub type Sections = Vec<(B, Vars)>;

#[cfg(feature = "lua")]
impl super::inipp_lua::Host for Parser {
    fn read(&self, path: &[u8]) -> Option<B> {
        self.read.as_ref().and_then(|r| r(path))
    }

    fn find(&self, sec: &super::inipp_lua::Pat, key: Option<&super::inipp_lua::Pat>, val: Option<&super::inipp_lua::Pat>) -> Option<Vec<B>> {
        let hit = |v: &Vec<B>| val.is_none_or(|p| v.iter().any(|x| p.hit(x)));
        self.done.iter().filter(|s| sec.hit(&s.name)).find_map(|s| match key {
            None => Some(Vec::new()),
            Some(k) => s.keys.iter().find(|(n, v)| k.hit(n) && hit(v)).map(|(_, v)| v.clone()),
        })
    }
}

/// One config as CSP's car and track loaders read it (ini ctor sub_181588210: mode 0, Lua on).
#[cfg(feature = "lua")]
pub struct Load<'a> {
    /// [`super::inipp_lua::ini_std`]
    pub std: &'a [u8],
    /// include folders to start with (ini +136)
    pub dirs: Vec<PathBuf>,
    /// files read after it by the same parser (ini +104: the generic config)
    pub after: Vec<PathBuf>,
    /// where `PREFIX_...` section numbers start
    pub base: i64,
    pub read: Reader,
}

/// `path` read as `o` says; `None` if it can't be read or Lua can't start.
#[cfg(feature = "lua")]
pub fn load(path: &Path, o: Load) -> Option<Sections> {
    let text = std::fs::read(path).ok()?;
    let mut p = Parser { file: path.to_path_buf(), lua: Some(Lua::new(o.std)?), dirs: o.dirs, read: Some(o.read), base: o.base, ..Default::default() };
    p.seen.push((path.to_path_buf(), Vec::new()));
    p.scan(strip_bom(&text), &mut vec![Vec::new()]);
    for f in o.after {
        let Ok(text) = std::fs::read(&f) else { continue };
        p.file = f.clone();
        p.seen.push((f, Vec::new()));
        p.scan(strip_bom(&text), &mut vec![Vec::new()]);
    }
    Some(p.resolve().into_iter().map(|s| (s.name, s.keys)).collect())
}

/// `text` as CSP reads a string with no file or include folders, Lua off: `ac.INIConfig.parse` in
/// its extended formats (sub_181588550; ini ctor sub_181490F80 mode 0).
pub fn parse(text: &[u8]) -> Sections {
    parse_with(text, None)
}

/// The same with Lua on and no `read()` data: a car's data file for `ac.INIConfig.carData`
/// (sub_181507A10, ini ctor sub_181588210 mode 0); `None` if Lua can't start.
#[cfg(feature = "lua")]
pub fn parse_lua(text: &[u8], std: &[u8]) -> Option<Sections> {
    Some(parse_with(text, Some(Lua::new(std)?)))
}

fn parse_with(text: &[u8], lua: Option<Lua>) -> Sections {
    let mut p = Parser { lua, ..Default::default() };
    p.scan(strip_bom(text), &mut vec![Vec::new()]);
    p.resolve().into_iter().map(|s| (s.name, s.keys)).collect()
}

/// The flattened text of resolved sections (as [`flatten`] gives).
pub fn text(sections: &Sections) -> String {
    let secs: Vec<Section> = sections.iter().map(|(n, k)| Section { name: n.clone(), keys: k.clone(), ..Default::default() }).collect();
    String::from_utf8_lossy(&write(&secs)).into_owned()
}

/// CSP's flattened text of the ini at `path` with Lua off, or `None` if it can't be read.
pub fn flatten(path: &Path) -> Option<String> {
    flatten_with(path, None)
}

/// The same with Lua on, CSP's normal mode; `std` is [`super::inipp_lua::ini_std`].
#[cfg(feature = "lua")]
pub fn flatten_lua(path: &Path, std: &[u8]) -> Option<String> {
    flatten_with(path, Some(Lua::new(std)?))
}

fn flatten_with(path: &Path, lua: Option<Lua>) -> Option<String> {
    let text = std::fs::read(path).ok()?;
    let mut p = Parser { file: path.to_path_buf(), lua, ..Default::default() };
    p.seen.push((path.to_path_buf(), Vec::new()));
    p.scan(strip_bom(&text), &mut vec![Vec::new()]);
    let sections = p.resolve();
    Some(String::from_utf8_lossy(&write(&sections)).into_owned())
}

fn strip_bom(t: &[u8]) -> &[u8] {
    t.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(t)
}

fn is_word(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric()
}

fn trim(s: &[u8]) -> &[u8] {
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let a = s.iter().position(|c| !ws(c)).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !ws(c)).map_or(a, |i| i + 1);
    &s[a..b]
}

fn find(s: &[u8], pat: &[u8]) -> Option<usize> {
    s.windows(pat.len()).position(|w| w == pat)
}

/// bool of a value (sub_1802C5520): starts with "true" (any case), else an integer != 0.
pub(crate) fn truthy(v: Option<&B>) -> bool {
    let Some(v) = v else { return false };
    if v.is_empty() || v[0] == 0 {
        return false;
    }
    if v.len() >= 4 && v[..4].eq_ignore_ascii_case(b"true") {
        return true;
    }
    atoi(v) != 0
}

fn atoi(v: &[u8]) -> i64 {
    let s = trim(v);
    let (neg, d) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let n = d.iter().take_while(|c| c.is_ascii_digit()).fold(0i64, |n, c| n.wrapping_mul(10).wrapping_add(i64::from(c - b'0')));
    if neg { -n } else { n }
}

/// A whole number or nothing (sub_1815D58E0).
fn parse_int(v: &[u8]) -> Option<i64> {
    let d = v.strip_prefix(b"-").or_else(|| v.strip_prefix(b"+")).unwrap_or(v);
    if d.is_empty() || !d.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = atoi(v);
    Some(n)
}

fn num(v: Option<&B>) -> f64 {
    v.and_then(|v| std::str::from_utf8(trim(v)).ok()).and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

impl Parser {
    /// Run Lua with this parser answering `read`, `has` and `get`.
    #[cfg(feature = "lua")]
    fn host<R>(&self, f: impl FnOnce() -> R) -> R {
        super::inipp_lua::with_host(self, f)
    }
    #[cfg(not(feature = "lua"))]
    fn host<R>(&self, f: impl FnOnce() -> R) -> R {
        f()
    }

    /// The line scanner (sub_181499CE0) over one file's text; `vars` is the scope chain.
    fn scan(&mut self, t: &[u8], vars: &mut Vec<Vars>) {
        vars.push(Vec::new());
        let n = t.len();
        let mut cur: Vec<Section> = Vec::new();
        let mut key: Option<B> = None;
        let (mut vstart, mut vend, mut quote): (i64, i64, i32) = (-1, -1, -1);
        let (mut data, mut comment) = (false, false);
        let at = |i: i64| if i >= 0 && (i as usize) < n { t[i as usize] } else { 0 };
        let mut i = 0usize;
        while i < n {
            let c = t[i];
            let ii = i as i64;
            if comment {
                comment = c != b'\n';
            } else if !matches!(c, b' ' | b'\t' | b'\r') && (quote == -1 || i32::from(c) == quote) {
                let commit = |p: &mut Parser, cur: &mut Vec<Section>, key: &Option<B>, vars: &mut Vec<Vars>| {
                    if let Some(k) = key {
                        for s in cur.iter_mut() {
                            p.commit(s, t, vstart, vend, k, vars);
                        }
                    }
                };
                if c == b'\n' && !(vend > 0 && at(vend) == b'\\') {
                    commit(self, &mut cur, &key, vars);
                    (key, vstart, quote, data) = (None, -1, -1, false);
                } else if data {
                    vend = ii;
                } else if c == b';' || (c == b'/' && i + 1 < n && t[i + 1] == b'/' && !url_before(t, i)) {
                    commit(self, &mut cur, &key, vars);
                    (key, vstart, quote, data, comment) = (None, -1, -1, false, true);
                } else if c == b'[' {
                    commit(self, &mut cur, &key, vars);
                    for s in cur.iter_mut() {
                        self.finalize(s, vars);
                    }
                    (key, vstart, quote, data) = (None, -1, -1, false);
                    i += 1;
                    if i == n {
                        // shortcut: CSP keeps the finalized sections and finalizes them again at the end
                        cur.clear();
                        break;
                    }
                    let s = i;
                    while i < n && t[i] != b']' {
                        i += 1;
                    }
                    cur = self.header(&t[s..i], vars);
                } else if c == b'=' {
                    if vstart != -1 && key.is_none() && !cur.is_empty() {
                        let len = (vend - vstart + 1).max(0) as usize;
                        key = Some(t[vstart as usize..vstart as usize + len].to_vec());
                        (vstart, quote, data) = (-1, -1, false);
                    }
                } else {
                    if (c == b'"' || c == b'\'') && key.is_some() && i > 1 {
                        if i32::from(c) == quote && (t[i - 1] != b'\\' || t[i - 2] == b'\\') {
                            quote = -1;
                        } else if quote == -1 {
                            let mut open = true;
                            if vstart != -1 {
                                let mut j = ii - 1;
                                while j >= vstart && matches!(at(j), b' ' | b'\t' | b'\r') {
                                    j -= 1;
                                }
                                if j >= vstart {
                                    open = match at(j) {
                                        b'$' => j == vstart || at(prev_non_ws(t, j - 1, vstart)) == b',',
                                        b',' => !(at(j - 1) == b'\\' && at(j - 2) != b'\\'),
                                        _ => false,
                                    };
                                }
                            }
                            if open {
                                quote = i32::from(c);
                                if vstart == -1 {
                                    (vstart, data) = (ii, false);
                                }
                            }
                        }
                    }
                    vend = ii;
                    if vstart == -1 {
                        vstart = ii;
                        data = t[i..].starts_with(b"data:image/png;base64,");
                    }
                }
            }
            i += 1;
        }
        if let Some(k) = &key {
            for s in cur.iter_mut() {
                self.commit(s, t, vstart, vend, k, vars);
            }
        }
        for s in cur.iter_mut() {
            self.finalize(s, vars);
        }
        vars.pop();
    }

    /// `[...]` (sub_181498920): the sections the following keys go to.
    fn header(&mut self, h: &[u8], vars: &[Vars]) -> Vec<Section> {
        let (prefix, rest) = match h.iter().position(|&c| c == b':') {
            Some(p) => (trim(&h[..p]).to_vec(), &h[p + 1..]),
            None => (Vec::new(), h),
        };
        let has_colon = h.contains(&b':');
        match prefix.as_slice() {
            b"INCLUDE" if has_colon => {
                let mut s = Section::named(b"INCLUDE");
                s.set(b"INCLUDE".to_vec(), split(trim(rest), false));
                return vec![s];
            }
            b"FUNCTION" if has_colon => {
                let mut s = Section::named(b"FUNCTION");
                s.set(b"NAME".to_vec(), vec![trim(rest).to_vec()]);
                return vec![s];
            }
            b"USE" if has_colon => {
                let mut s = Section::named(b"USE");
                s.set(b"FILE".to_vec(), vec![trim(rest).to_vec()]);
                return vec![s];
            }
            b"TEMPLATE" | b"MIXIN" if has_colon => {
                let words: Vec<&[u8]> = rest.split(|&c| c == b' ').filter(|w| !w.is_empty()).collect();
                let mut t = Template::default();
                let mut words = words.as_slice();
                if prefix == b"TEMPLATE" {
                    if let Some(l) = words.last() {
                        if words.len() > 1 && (*l == b"earlyresolve" || *l == b"EARLYRESOLVE") {
                            t.early = true;
                            words = &words[..words.len() - 1];
                        }
                    }
                }
                let name = words.first().map_or(Vec::new(), |w| w.to_vec());
                if words.len() > 2 && (words[1] == b"extends" || words[1] == b"EXTENDS") {
                    for w in &words[2..] {
                        let w = trim_set(w, b", \t\r");
                        if !w.is_empty() {
                            t.parents.push(w.to_vec());
                        }
                    }
                }
                // get-or-create (sub_181490D20): a later `[TEMPLATE: X]` adds to X; parents are created too
                let list = if prefix == b"TEMPLATE" { &mut self.templates } else { &mut self.mixins };
                for p in &t.parents {
                    if !list.iter().any(|(n, _)| n == p) {
                        list.push((p.clone(), Template { scope: vars.to_vec(), ..Default::default() }));
                    }
                }
                match list.iter_mut().find(|(n, _)| *n == name) {
                    Some(e) => {
                        e.1.early |= t.early;
                        e.1.parents.extend(t.parents);
                    }
                    None => list.push((name.clone(), Template { scope: vars.to_vec(), ..t })),
                }
                let mut s = Section::named(&name);
                s.tpl = Some(if prefix == b"TEMPLATE" { name } else { [b"\0".as_slice(), &name].concat() });
                return vec![s];
            }
            _ => {}
        }
        let names: Vec<B> = rest.split(|&c| c == b',').map(|n| trim(n).to_vec()).collect();
        // names are looked up last to first; each template is followed by its parents (sub_1814988A0)
        let (mut early, mut late) = (Vec::new(), Vec::new());
        for n in names.iter().rev() {
            self.template_chain(n, &mut early, &mut late);
        }
        let mut out: Vec<Section> = if late.is_empty() {
            names.iter().map(|n| Section::named(n)).collect()
        } else {
            vec![Section { name: prefix, tpls: late, ..Default::default() }]
        };
        for s in &mut out {
            for t in &early {
                self.apply(s, t, vars, &Vec::new(), true);
            }
        }
        out
    }

    fn template_chain(&self, name: &[u8], early: &mut Vec<B>, late: &mut Vec<B>) {
        if let Some((_, t)) = self.templates.iter().find(|(n, _)| n == name) {
            if t.early { &mut *early } else { &mut *late }.push(name.to_vec());
            for p in &t.parents {
                self.template_chain(p, early, late);
            }
        }
    }

    /// One template's keys into a section (sub_181494190): the section's own keys stay, `@OUTPUT`
    /// names an unnamed section. Variables, first found wins: TARGET, the section's keys, the
    /// scopes here, the template's own.
    /// `tname` starting with 0 is a mixin (sub_181495130: `args` above everything, `protect`
    /// off at a key's commit, so it overwrites the section's keys).
    fn apply(&mut self, sec: &mut Section, tname: &[u8], vars: &[Vars], args: &Vars, protect: bool) {
        let list = if tname.first() == Some(&0) { &self.mixins } else { &self.templates };
        let name = tname.strip_prefix(b"\0").unwrap_or(tname);
        let Some(t) = list.iter().find(|(n, _)| n == name).map(|(_, t)| t.clone()) else { return };
        let mut sc = t.scope.clone();
        sc.extend(vars.iter().cloned());
        let mut target: Vars = Vec::new();
        if !sec.name.is_empty() {
            target.push((b"TARGET".to_vec(), vec![sec.name.clone()]));
        }
        let get = |k: &[u8]| t.keys.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let scope = |sec: &Section, target: &Vars| -> Vec<Vars> {
            let mut s = sc.clone();
            s.extend([sec.keys.clone(), target.clone(), args.clone()]);
            s
        };
        let s0 = scope(sec, &target);
        if get(b"@ACTIVE").and_then(|v| self.expand(b"@ACTIVE", v, &s0)).is_some_and(|v| !truthy(v.first())) {
            return;
        }
        if get(b"@DISCARD").and_then(|v| self.expand(b"@DISCARD", v, &s0)).is_some_and(|v| truthy(v.first())) {
            return;
        }
        let mut discard = false;
        let mut done = sec.rec.iter().find(|(n, _)| n == tname).map(|(_, v)| v.clone()).unwrap_or_default();
        for (k, v) in &t.keys {
            let output = k == b"@OUTPUT";
            if k.starts_with(b"@ACTIVE") || k.starts_with(b"@DISCARD") || (output && !sec.name.is_empty()) {
                continue;
            }
            if k.starts_with(b"@GENERATOR") {
                let mut s = sc.clone();
                s.extend([target.clone(), args.clone()]);
                if let Some(items) = self.expand(k, v.clone(), &scope(sec, &target)) {
                    self.generator(sec, &items, &s, args);
                }
                continue;
            }
            if k == b"@" || k.starts_with(b"@MIXIN") {
                let mut s = sc.clone();
                s.extend([target.clone(), args.clone()]);
                if let Some(items) = self.expand(k, v.clone(), &scope(sec, &target)) {
                    self.mixin(sec, &items, &s, args, protect);
                }
                continue;
            }
            if protect && !output && sec.get(k).is_some() && !done.contains(k) {
                continue;
            }
            let s = scope(sec, &target);
            let Some(items) = self.expand(k, v.clone(), &s) else {
                discard |= output;
                continue;
            };
            if output {
                sec.name = items.first().cloned().unwrap_or_default();
                target = vec![(b"TARGET".to_vec(), vec![sec.name.clone()])];
                discard = false;
                continue;
            }
            let mut key = k.clone();
            if find(k, b"${").is_some() || find(k, b"$\"").is_some() || find(k, LUA_MARK).is_some() {
                // a key the template line already marked as Lua is one item (its commas are the code's)
                let ks = if k.starts_with(LUA_MARK) { vec![k.clone()] } else { split(k, k[0] == b'@') };
                match self.expand(k, ks, &s) {
                    Some(n) => key = n.into_iter().next().unwrap_or_default(),
                    None => return,
                }
            }
            let key = self.key_fix(key);
            if protect {
                done.push(key.clone());
            }
            sec.set(key, items);
        }
        match sec.rec.iter_mut().find(|(n, _)| n == tname) {
            Some(e) => e.1 = done,
            None => sec.rec.push((tname.to_vec(), done)),
        }
        if discard {
            sec.set(b"@DISCARD".to_vec(), vec![b"1".to_vec()]);
        }
    }

    /// `@ = NAME, ARG = value, ...` (sub_1814952E0): the mixin's keys with the arguments as variables.
    /// `outer`: the calling mixin's arguments, which win over the section's keys here too.
    fn mixin(&mut self, sec: &mut Section, items: &[B], vars: &[Vars], outer: &Vars, protect: bool) {
        let Some(name) = items.first().filter(|n| !n.is_empty()) else { return };
        let mut s = vars.to_vec();
        s.extend([sec.keys.clone(), outer.clone()]);
        let mut args = outer.clone();
        for it in &items[1..] {
            // a bare `FLAG` is 1, as for @GENERATOR (sub_181492970)
            let Some(e) = it.iter().position(|&c| c == b'=') else {
                // shortcut: a bare number (a generator's count) is skipped in a mixin, upgrade if a config passes one
                if !trim(it).is_empty() && !trim(it)[0].is_ascii_digit() {
                    set_var(&mut args, trim(it).to_vec(), vec![b"1".to_vec()]);
                }
                continue;
            };
            let v = self.expand(b"", split(trim(&it[e + 1..]), false), &s).unwrap_or_default();
            set_var(&mut args, trim(&it[..e]).to_vec(), v);
        }
        self.apply(sec, &[b"\0".as_slice(), name].concat(), vars, &args, protect);
    }

    /// `@GENERATOR = TEMPLATE, [count, ...], [FLAG | ARG = value, ...]` (sub_181493780): a new
    /// section from the template (and its parents) per combination of the counts, `$1`, `$2`, ...
    /// from 1, the last fastest; stored at once, before the section naming it; the host's keys are
    /// variables, a bare `FLAG` is 1; no `@OUTPUT`: the unnamed section. A missing template is only logged.
    fn generator(&mut self, host: &Section, items: &[B], vars: &[Vars], outer: &Vars) {
        let Some(name) = items.first().filter(|n| !n.is_empty()) else { return };
        let mut s = vars.to_vec();
        s.extend([host.keys.clone(), outer.clone()]);
        let (mut args, mut counts): (Vars, Vec<i64>) = (outer.clone(), Vec::new());
        for it in &items[1..] {
            match it.iter().position(|&c| c == b'=') {
                Some(e) => {
                    let v = self.expand(b"", split(trim(&it[e + 1..]), false), &s).unwrap_or_default();
                    set_var(&mut args, trim(&it[..e]).to_vec(), v);
                }
                None if trim(it).first().is_some_and(u8::is_ascii_digit) => counts.push(atoi(trim(it))),
                None => set_var(&mut args, trim(it).to_vec(), vec![b"1".to_vec()]),
            }
        }
        let (mut chain, mut late) = (Vec::new(), Vec::new());
        self.template_chain(name, &mut chain, &mut late);
        chain.extend(late);
        let mut idx = vec![1i64; counts.len()];
        loop {
            let mut a = args.clone();
            for (i, v) in idx.iter().enumerate() {
                set_var(&mut a, (i + 1).to_string().into_bytes(), vec![v.to_string().into_bytes()]);
            }
            let mut sec = Section::default();
            for t in &chain {
                self.apply(&mut sec, t, &s, &a, true);
            }
            self.finalize(&mut sec, &mut s.clone());
            let mut i = counts.len();
            loop {
                if i == 0 {
                    return;
                }
                i -= 1;
                idx[i] += 1;
                if idx[i] <= counts[i] {
                    break;
                }
                idx[i] = 1;
            }
        }
    }

    /// One `KEY=value` into a section (sub_1814973D0).
    fn commit(&mut self, sec: &mut Section, t: &[u8], vstart: i64, vend: i64, key: &[u8], vars: &mut [Vars]) {
        if key.is_empty() {
            return;
        }
        let raw: &[u8] = if vstart == -1 {
            &[]
        } else {
            let len = (vend - vstart + 1).max(0) as usize;
            &t[vstart as usize..(vstart as usize + len).min(t.len())]
        };
        let at_key = key[0] == b'@';
        let mut items = split(raw, at_key);
        let plain = sec.tpl.is_some() || sec.name == b"DEFAULTS" || sec.name == b"INCLUDE";
        // the section's keys so far are variables too, over [DEFAULTS]
        let mut scope = vars.to_vec();
        scope.push(sec.keys.clone());
        if !plain {
            match self.expand(key, items, &scope) {
                Some(v) => items = v,
                None => return,
            }
        }
        let mut key = key.to_vec();
        if let Some(d) = key.iter().position(|&c| c == b'$') {
            if matches!(key.get(d + 1), Some(b'{' | b'"')) {
                let k = split(&key, at_key);
                let k = if plain { Some(k) } else { self.expand(&key, k, &scope) };
                match k {
                    Some(k) => key = k.into_iter().next().unwrap_or_default(),
                    None => return,
                }
            }
        }
        if let Some(tn) = &sec.tpl {
            let (list, name) = match tn.strip_prefix(b"\0") {
                Some(m) => (&mut self.mixins, m),
                None => (&mut self.templates, tn.as_slice()),
            };
            if let Some((_, tp)) = list.iter_mut().find(|(n, _)| n == name) {
                // a repeated key is another line of CSP's template, applied in order (a `discard()` skips only its line)
                tp.keys.push((key, items));
            }
            return;
        }
        if key == b"@" || key.starts_with(b"@MIXIN") {
            self.mixin(sec, &items, vars, &Vec::new(), false);
            return;
        }
        if key.starts_with(b"@GENERATOR") {
            self.generator(sec, &items, vars, &Vec::new());
            return;
        }
        if key == b"@ERASE_REFERENCED" && sec.name == b"@INIPP" {
            return;
        }
        if sec.name == b"DEFAULTS" {
            let (name, vals) = if key.starts_with(b"VAR") && !items.is_empty() {
                let name = items[0].clone();
                (name, if items.len() > 1 { items[1..].to_vec() } else { Vec::new() })
            } else {
                (key, items)
            };
            if let Some(top) = vars.last_mut().filter(|_| !self.given.contains(&name)) {
                set_var(top, name, vals);
            }
            return;
        }
        if sec.name == b"INCLUDE" && key == b"INCLUDE" {
            let mut list = sec.get(b"INCLUDE").cloned().unwrap_or_default();
            list.extend(items);
            sec.set(key, list);
            return;
        }
        let key = self.key_fix(key);
        sec.set(key, items);
    }

    /// `PREFIX_...` keys (sub_181498310): numbered later, in the order of a running counter.
    fn key_fix(&mut self, key: B) -> B {
        if key.len() >= 4 && (key.ends_with(b"_...") || key.ends_with("_\u{2026}".as_bytes())) {
            let mut k = key[..key.len() - 3].to_vec();
            k.extend_from_slice(KEY_MARK);
            k.extend_from_slice(self.counter.to_string().as_bytes());
            self.counter += 1;
            return k;
        }
        key
    }

    /// The end of a section (sub_181495400): `@DISCARD`, `[INCLUDE]`, then stored unless empty.
    fn finalize(&mut self, sec: &mut Section, vars: &mut Vec<Vars>) {
        if sec.tpl.is_some() {
            return;
        }
        for t in sec.tpls.clone() {
            self.apply(sec, &t, vars, &Vec::new(), true);
        }
        if let Some(v) = sec.get(b"@DISCARD") {
            if truthy(v.first()) {
                return;
            }
            sec.remove(b"@DISCARD");
        }
        // inactive FUNCTION/USE/INCLUDE vanish (a `$"..."` ACTIVE is false with Lua off); others
        // keep their keys with Lua off, only `ACTIVE=0` with Lua on
        let special = matches!(&sec.name[..], b"FUNCTION" | b"USE" | b"INCLUDE");
        // a still marked ACTIVE (unexpanded `[INCLUDE]` keys) is run here
        let active = match sec.get(b"ACTIVE").cloned() {
            Some(v) if v.iter().any(|i| find(i, LUA_MARK).is_some()) => Some(self.expand(b"ACTIVE", v, vars).unwrap_or_default()),
            v => v,
        };
        if active.is_some_and(|v| !truthy(v.first())) {
            if special {
                return;
            }
            if self.lua.is_some() {
                sec.keys = vec![(b"ACTIVE".to_vec(), vec![b"0".to_vec()])];
            }
        }
        let first = |k: &[u8]| sec.get(k).and_then(|v| v.first()).cloned().unwrap_or_default();
        match (&self.lua, &sec.name[..]) {
            (Some(lua), b"FUNCTION") => {
                self.host(|| lua.function(&first(b"NAME"), sec.get(b"ARGUMENTS").map_or(&[][..], |v| v), &first(b"CODE")));
                return;
            }
            (Some(_), b"USE") => {
                if let Some(path) = self.resolve_file(&first(b"FILE")) {
                    self.host(|| self.lua.as_ref().expect("Lua on").use_file(&path));
                }
                return;
            }
            _ => {}
        }
        if sec.name.starts_with(b"INCLUDE") {
            if let Some(files) = sec.get(b"INCLUDE").cloned() {
                self.include(sec, files, vars);
                return;
            }
        }
        if !sec.keys.is_empty() {
            self.done.push(sec.clone());
        }
    }

    /// `[INCLUDE]`: the section's other keys are variables of the included files.
    fn include(&mut self, sec: &Section, files: Vec<B>, vars: &mut Vec<Vars>) {
        let mut child: Vars = Vec::new();
        for (k, v) in &sec.keys {
            if k == b"INCLUDE" {
                continue;
            }
            if k.starts_with(b"VAR") {
                let name = v.first().cloned().unwrap_or_default();
                set_var(&mut child, name, v.iter().skip(1).cloned().collect());
            } else {
                set_var(&mut child, k.clone(), v.clone());
            }
        }
        let Some(files) = self.expand(b"INCLUDE", files, vars) else { return };
        let saved = self.file.clone();
        for f in files {
            let Some(path) = self.resolve_file(&f) else { continue };
            if self.seen.iter().any(|(p, v)| *p == path && *v == child) {
                continue;
            }
            self.seen.push((path.clone(), child.clone()));
            let Ok(text) = std::fs::read(&path) else { continue };
            self.file = path;
            vars.push(child.clone());
            let given = self.given.len();
            self.given.extend(child.iter().map(|(n, _)| n.clone()));
            self.scan(strip_bom(&text), vars);
            self.given.truncate(given);
            vars.pop();
            self.file = saved.clone();
        }
    }

    /// An include's path (sub_1814915E0): next to the current file, then the folders found so far.
    fn resolve_file(&mut self, f: &[u8]) -> Option<PathBuf> {
        let rel = PathBuf::from(String::from_utf8_lossy(f).replace('/', "\\"));
        let here = self.file.parent().map(Path::to_path_buf).unwrap_or_default();
        let bases: Vec<PathBuf> = std::iter::once(here).chain(self.dirs.iter().cloned()).collect();
        for base in bases {
            let p = base.join(&rel);
            if p.is_file() {
                let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
                if dir != base && !self.dirs.contains(&dir) {
                    self.dirs.push(dir);
                }
                return Some(p);
            }
        }
        None
    }

    /// Variables in a value's items (sub_181491970); `None`: a required one is missing.
    fn expand(&self, key: &[u8], items: Vec<B>, vars: &[Vars]) -> Option<Vec<B>> {
        let raw_eq = key == b"@" || key.starts_with(b"@MIXIN") || key.starts_with(b"@GENERATOR");
        let mut out = Vec::new();
        let mut ok = true;
        for it in items {
            // an `ARG = value` stays raw for the mixin; a `$"..."` name with `==` in it is no argument
            if raw_eq && it.contains(&b'=') && !it.starts_with(LUA_MARK) {
                out.push(it);
            } else {
                self.expand_item(&it, 0, vars, &mut out, &mut ok);
            }
        }
        ok.then_some(out)
    }

    /// One item (sub_18148FEB0).
    fn expand_item(&self, s: &[u8], depth: u32, vars: &[Vars], out: &mut Vec<B>, ok: &mut bool) {
        if depth >= 100 {
            out.push(s.to_vec());
            return;
        }
        // a `$"..."` Lua expression: no item at all with Lua off; with Lua, references in it
        // become Lua literals (sub_18148E3A0's Lua flag) and it is run at the end
        let lua = s.starts_with(LUA_MARK);
        if lua && self.lua.is_none() {
            return;
        }
        if let Some(r) = whole_ref(s) {
            for v in self.lookup(&r, s, vars, ok, false) {
                self.expand_item(&v, depth + 1, vars, out, ok);
            }
            return;
        }
        let mut refs = |r: Ref, a: usize, b: usize| {
            let vals = self.lookup(&r, &s[a..b], vars, ok, lua);
            let vals = if lua { vec![literal(r.modi, &vals)] } else { vals };
            for v in vals {
                self.expand_item(&[&s[..a], &v[..], &s[b..]].concat(), depth + 1, vars, out, ok);
            }
        };
        if let Some(p) = find(s, b"${") {
            if let Some(q) = s[p..].iter().position(|&c| c == b'}').map(|q| q + p) {
                if let Some(r) = whole_ref(&s[p..=q]) {
                    return refs(r, p, q + 1);
                }
            }
        }
        if let Some(d) = s.iter().skip(1).position(|&c| c == b'$').map(|d| d + 1) {
            let e = d + 1 + s[d + 1..].iter().take_while(|&&c| is_word(c)).count();
            if e > d + 1 {
                return refs(whole_ref(&s[d..e]).expect("a $name reference"), d, e);
            }
        }
        if lua {
            self.run_lua(s, out, ok);
        } else {
            out.push(s.to_vec());
        }
    }

    /// A Lua item's values (sub_18148BF40/sub_18148BB50): the last expression in it is run,
    /// the text around it kept on each value; again while the text has one.
    fn run_lua(&self, s: &[u8], out: &mut Vec<B>, ok: &mut bool) {
        let lua = self.lua.as_ref().expect("Lua on");
        let open = s.windows(LUA_MARK.len()).rposition(|w| w == LUA_MARK);
        let Some((o, c)) = open.and_then(|o| Some((o, find(&s[o..], LUA_END)? + o))) else {
            out.push(s.to_vec());
            return;
        };
        let (prefix, expr, suffix) = (&s[..o], &s[o + LUA_MARK.len()..c], &s[c + LUA_END.len()..]);
        if find(prefix, LUA_MARK).is_none() {
            *ok &= self.host(|| lua.eval(expr, prefix, suffix, out));
            return;
        }
        let mut vals = Vec::new();
        *ok &= self.host(|| lua.eval(expr, prefix, suffix, &mut vals));
        for v in vals {
            self.run_lua(&v, out, ok);
        }
    }

    /// A reference's values (sub_18148D630 + sub_18148C380); `text`: the reference as written,
    /// what a missing `$name` gives unless it's in a Lua expression (`lua`).
    fn lookup(&self, r: &Ref, text: &[u8], vars: &[Vars], ok: &mut bool, lua: bool) -> Vec<B> {
        let found = vars.iter().rev().find_map(|sc| sc.iter().find(|(n, _)| *n == r.name).map(|(_, v)| v));
        let zeros = |n: usize| vec![b"0".to_vec(); n];
        let Some(items) = found else {
            if r.required {
                *ok = false;
            }
            if !r.default.is_empty() {
                return vec![r.default.clone()];
            }
            return match r.modi {
                1 | 2 | 3 | 11 => zeros(1),
                4 => zeros(2),
                5 => zeros(3),
                6 => zeros(4),
                12 => vec![b"false".to_vec()],
                13 => vec![Vec::new()],
                // missing: `$name` stays as written, `${name}` gives nothing
                _ if r.simple && !lua => vec![text.to_vec()],
                _ => Vec::new(),
            };
        };
        let len = items.len() as i64;
        let (mut start, mut end) = (r.start, r.end);
        if end < 0 || (end == 0 && start < 0) {
            end += len;
        }
        if start < 0 {
            start += len;
        }
        let have = if len == 1 { i64::from(!items[0].is_empty()) } else { len };
        let count = (end - start).min(have - start);
        if count <= 0 && r.required {
            *ok = false;
        }
        let range = || (start.max(0)..have.min(end).max(0)).map(|i| &items[i as usize]);
        let item = |i: i64| if i >= 0 && i < len { Some(&items[i as usize]) } else { None };
        match r.modi {
            0 => range().cloned().collect(),
            1 => vec![count.to_string().into_bytes()],
            2 => vec![range().map(Vec::len).sum::<usize>().to_string().into_bytes()],
            3 => vec![if start < have && start < end { b"1".to_vec() } else { b"0".to_vec() }],
            4..=6 => {
                let want = usize::from(r.modi - 2);
                let mut v: Vec<B> = Vec::new();
                for it in range() {
                    v.push(if is_number(it) { it.clone() } else { b"0".to_vec() });
                    if v.len() >= want {
                        break;
                    }
                }
                let fill = if v.len() == 1 { v[0].clone() } else { b"0".to_vec() };
                while v.len() < want {
                    v.push(fill.clone());
                }
                v
            }
            7..=10 => {
                let k = i64::from(r.modi - 7);
                if k < count {
                    vec![item(start + k).cloned().unwrap_or_default()]
                } else if count == 1 {
                    vec![item(start).cloned().unwrap_or_default()]
                } else {
                    zeros(1)
                }
            }
            11 => vec![format!("{:.6}", num(item(start))).into_bytes()],
            12 => vec![if truthy(item(start)) { b"true".to_vec() } else { b"false".to_vec() }],
            _ => vec![item(start).cloned().unwrap_or_default()],
        }
    }

    /// End of the load (sub_18149B1C0): `PREFIX_...` sections numbered, same names merged, then
    /// each section's `PREFIX_...` keys numbered (sub_18149A860).
    fn resolve(&mut self) -> Vec<Section> {
        // prefix -> (next number, numbers taken by explicit `PREFIX_<n>` names)
        let mut nums: Vec<(B, i64, Vec<i64>)> = Vec::new();
        // sub_18149B080: car lights keep numbering from 0
        let lights: [&[u8]; 8] = [b"HEADLIGHT", b"BRAKE", b"REVERSE", b"KERS", b"OPENDOORS", b"LED_PANEL", b"TURNSIGNAL", b"CORNERINGLAMP"];
        let from = self.base;
        let base = |p: &[u8]| if p.strip_prefix(b"LIGHT_").is_some_and(|r| lights.iter().any(|l| r.starts_with(l))) { 0 } else { from };
        let state = |nums: &mut Vec<(B, i64, Vec<i64>)>, p: &[u8]| -> usize {
            nums.iter().position(|(n, _, _)| n == p).unwrap_or_else(|| {
                nums.push((p.to_vec(), base(p), Vec::new()));
                nums.len() - 1
            })
        };
        for s in &self.done {
            let n = &s.name;
            if n.len() < 3 {
                continue;
            }
            let digits = n.iter().rev().take(n.len() - 2).take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && n[n.len() - 1 - digits] == b'_' {
                let i = state(&mut nums, &n[..n.len() - digits]);
                let v = atoi(&n[n.len() - digits..]);
                if v == nums[i].1 {
                    nums[i].1 += 1;
                } else {
                    nums[i].2.push(v);
                }
            }
        }
        let mut merged: Vec<Section> = Vec::new();
        for s in std::mem::take(&mut self.done) {
            let mut s = s;
            let n = &s.name;
            if n.len() >= 4 && (n.ends_with(b"_...") || n.ends_with("_\u{2026}".as_bytes())) {
                let i = state(&mut nums, &n[..n.len() - 3]);
                let mut v = nums[i].1;
                nums[i].1 = v + 1;
                while nums[i].2.contains(&v) {
                    v = nums[i].1;
                    nums[i].1 = v + 1;
                }
                let mut name = n[..n.len() - 3].to_vec();
                name.extend_from_slice(v.to_string().as_bytes());
                s.name = name;
            }
            match merged.iter_mut().find(|m| m.name == s.name) {
                Some(m) => {
                    for (k, v) in s.keys {
                        m.set(k, v);
                    }
                }
                None => merged.push(s),
            }
        }
        for s in &mut merged {
            let mut input = std::mem::take(&mut s.keys);
            input.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out: Vars = Vec::new();
            for (k, v) in &input {
                match find(k, KEY_MARK) {
                    Some(p) => {
                        for n in 0..10000 {
                            let c = [&k[..p], n.to_string().as_bytes()].concat();
                            if !out.iter().any(|(o, _)| *o == c) && !input.iter().any(|(o, _)| *o == c) {
                                out.push((c, v.clone()));
                                break;
                            }
                        }
                    }
                    None => set_var(&mut out, k.clone(), v.clone()),
                }
            }
            s.keys = out;
        }
        merged
    }
}

fn trim_set<'a>(s: &'a [u8], set: &[u8]) -> &'a [u8] {
    let a = s.iter().position(|c| !set.contains(c)).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !set.contains(c)).map_or(a, |i| i + 1);
    &s[a..b]
}

fn prev_non_ws(t: &[u8], mut j: i64, lo: i64) -> i64 {
    while j >= lo && j >= 0 && matches!(t[j as usize], b' ' | b'\t' | b'\r') {
        j -= 1;
    }
    j
}

/// Is the `//` at `i` part of an `http://` or `https://` (then not a comment)?
fn url_before(t: &[u8], i: usize) -> bool {
    if i == 0 || t[i - 1] != b':' || i < 4 {
        return false;
    }
    let colon = i - 1;
    let start = if colon >= 4 && &t[colon - 4..colon] == b"http" {
        colon - 4
    } else if colon >= 5 && &t[colon - 5..colon - 1] == b"http" && t[colon - 1] == b's' {
        colon - 5
    } else {
        return false;
    };
    start != 0 && !t[start - 1].is_ascii_alphanumeric()
}

fn is_number(v: &[u8]) -> bool {
    std::str::from_utf8(trim(v)).ok().and_then(|s| s.parse::<f64>().ok()).is_some()
}

/// A reference's values as Lua source inside `$"..."` (sub_18148E3A0 Lua path, sub_18148C1C0).
fn literal(modi: u8, vals: &[B]) -> B {
    match vals {
        [] => b"nil".to_vec(),
        [v] => match modi {
            0 | 7..=10 if !csp_number(v) => quote(v),
            3 => (if truthy(Some(v)) { &b"true"[..] } else { b"false" }).to_vec(),
            13 => quote(v),
            _ => v.clone(),
        },
        _ if vals.len() <= 4 && vals.iter().all(|v| csp_number(v)) => [format!("vec{}(", vals.len()).as_bytes(), &vals.join(&b','), b")"].concat(),
        _ => {
            let items: Vec<B> = vals.iter().map(|v| if csp_number(v) { v.clone() } else { quote(v) }).collect();
            [&b"{"[..], &items.join(&b','), b"}"].concat()
        }
    }
}

/// A Lua string literal (sub_18148DB70).
fn quote(v: &[u8]) -> B {
    let mut o = vec![b'"'];
    for &c in v {
        match c {
            8 => o.extend_from_slice(b"\\b"),
            b'\t' => o.extend_from_slice(b"\\t"),
            b'\n' => o.extend_from_slice(b"\\n"),
            b'\r' => o.extend_from_slice(b"\\r"),
            b'"' | b'\\' => o.extend_from_slice(&[b'\\', c]),
            _ => o.push(c),
        }
    }
    o.push(b'"');
    o
}

/// Can the text go into Lua as a number as it is (sub_18148DA30)? Empty counts as one.
fn csp_number(v: &[u8]) -> bool {
    let blank = |c: u8| matches!(c, b' ' | b'\t' | b'\r');
    let a = v.iter().position(|&c| !blank(c)).unwrap_or(v.len());
    let b = v.iter().rposition(|&c| !blank(c)).map_or(a, |i| i + 1);
    let s = &v[a..b];
    let hex = s.len() >= 2 && s[0] == b'0' && s[1] == b'x';
    let (mut dot, mut exp, mut exp_digits) = (false, false, false);
    for (i, &c) in s.iter().enumerate().skip(if hex { 2 } else { 0 }) {
        match c {
            b'0'..=b'9' => exp_digits |= exp,
            b'+' | b'-' if i == 0 || (exp && matches!(s[i - 1], b'e' | b'E')) => {}
            b'e' | b'E' if !hex && !exp && i > 0 => exp = true,
            b'.' if !hex && !dot && !exp => dot = true,
            _ => return false,
        }
    }
    !exp || exp_digits
}

/// `$name` or `${name:...}` as a whole (sub_18148FCD0), `None` if it isn't one.
fn whole_ref(s: &[u8]) -> Option<Ref> {
    if s.len() < 2 || s[0] != b'$' {
        return None;
    }
    if s[1] == b'{' && s[s.len() - 1] == b'}' {
        return spec(&s[2..s.len() - 1]);
    }
    let name = &s[1..];
    name.iter().all(|&c| is_word(c)).then(|| Ref { name: name.to_vec(), simple: true, end: i64::from(i32::MAX), ..Default::default() })
}

/// The inside of `${...}`: `name[:index[:count]][:modifier...]` (sub_18148F100).
fn spec(s: &[u8]) -> Option<Ref> {
    let parts: Vec<&[u8]> = s.split(|&c| c == b':').collect();
    if parts.len() > 5 || parts[0].is_empty() || !parts[0].iter().all(|&c| is_word(c)) {
        return None;
    }
    let n = parts.len();
    let (mut idx, mut has_idx, mut count) = (1i64, false, 10000i64);
    if n > 1 {
        if let Some(v) = parse_int(parts[1]) {
            (idx, has_idx, count) = (v, true, 1);
        }
        if n > 2 {
            if let Some(v) = parse_int(parts[2]) {
                count = v;
            }
        }
    }
    let mut end = count + idx;
    if n > 3 && parts[2].is_empty() {
        end = if has_idx { idx + 1 } else { 10000 };
        if let Some(v) = parse_int(parts[3]) {
            end = v;
        }
    }
    let mut r = Ref { name: parts[0].to_vec(), ..Default::default() };
    for p in &parts[1..] {
        if p.is_empty() || !(p[0].is_ascii_alphabetic() || p[0] == b'?') {
            continue;
        }
        r.modi = match *p {
            b"size" | b"count" => 1,
            b"length" => 2,
            b"exists" => 3,
            b"vec2" => 4,
            b"vec3" => 5,
            b"vec4" => 6,
            b"x" => 7,
            b"y" => 8,
            b"z" => 9,
            b"w" => 10,
            b"num" | b"number" => 11,
            b"bool" | b"boolean" => 12,
            b"str" | b"string" => 13,
            _ => {
                if let Some(d) = p.strip_prefix(b"or=") {
                    r.default = d.to_vec();
                }
                r.modi
            }
        };
        if *p == b"required" || *p == b"?" {
            r.required = true;
        }
    }
    r.start = match idx {
        0 => 0,
        i if i > 0 => i - 1,
        i => i,
    };
    r.end = if end > 0 { end - 1 } else { end };
    Some(r)
}

/// A value's items (sub_1814965D0): commas, quotes, `\` escapes; `at`: the key starts with `@`.
fn split(t: &[u8], at: bool) -> Vec<B> {
    if t.starts_with(b"data:image/png;base64,") {
        return vec![t.to_vec()];
    }
    let n = t.len() as i64;
    let ch = |i: i64| if i >= 0 && i < n { t[i as usize] } else { 0 };
    let mut items: Vec<B> = Vec::new();
    let mut buf: B = Vec::new();
    let (mut q, mut dollar) = (0u8, false);
    // the item's text before the open quote (an `@` line's `ARG = $"..."`)
    let mut pre = 0;
    let (mut s, mut e) = (0i64, -1i64);
    let flush = |buf: &mut B, s: i64, e: i64| {
        if e >= s {
            buf.extend_from_slice(&t[s as usize..=e as usize]);
        }
    };
    let mut i = 0i64;
    while i < n {
        let c = t[i as usize];
        if c == b'\\' && i < n - 1 {
            let n1 = ch(i + 1);
            let take = if n1 == b'\n' || (n1 == b'\r' && i < n - 2 && ch(i + 2) == b'\n') {
                Some(true)
            } else if n1 == b'"' || n1 == b'\'' {
                if buf.is_empty() && e < s {
                    Some(false)
                } else if q != 0 {
                    Some(true)
                } else {
                    None
                }
            } else if q == 0 && n1 == b',' {
                Some(true)
            } else {
                None
            };
            if let Some(fl) = take {
                if fl {
                    flush(&mut buf, s, e);
                }
                i += 1 + i64::from(ch(i + 1) == b'\r');
                if ch(i) != b'\n' {
                    buf.push(ch(i));
                }
                (s, e) = (i + 1, i);
                i += 1;
                continue;
            }
        }
        if !dollar && q == b'"' && c == b'\\' {
            if i < n - 1 {
                let r = match ch(i + 1) {
                    b'\n' => Some(0),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    _ => None,
                };
                if let Some(r) = r {
                    flush(&mut buf, s, e);
                    if r != 0 {
                        buf.push(r);
                    }
                    i += 1;
                    s = i + 1;
                }
            }
            e = i;
        } else if c == b'"' || c == b'\'' {
            if q == c {
                flush(&mut buf, s, e);
                let mut j = i + 1;
                let mut last = true;
                while j < n {
                    let cj = ch(j);
                    if cj != b'\\' {
                        if cj == b',' {
                            break;
                        }
                        if !matches!(cj, b' ' | b'\t' | b'\r') {
                            last = false;
                            break;
                        }
                    }
                    j += 1;
                }
                if !last || (dollar && pre > 0) {
                    // text follows the closing quote, or an `ARG = $"..."` (split again by the mixin): the quotes stay
                    let mut nb = buf[..pre].to_vec();
                    if dollar {
                        nb.push(b'$');
                    }
                    nb.push(c);
                    nb.extend_from_slice(&buf[pre..]);
                    nb.push(c);
                    buf = nb;
                } else if dollar {
                    buf = [LUA_MARK, &buf, LUA_END].concat();
                }
                q = 0;
                (s, e) = (i + 1, i);
            } else if q != 0 || (!at && (!buf.is_empty() || e >= s)) {
                e = i;
            } else {
                flush(&mut buf, s, e);
                pre = buf.len();
                q = c;
                dollar = i > 0 && ch(i - 1) == b'$';
                (s, e) = (i + 1, i);
            }
        } else if q != 0 || c != b',' {
            let ws = matches!(c, b' ' | b'\t' | b'\r');
            if (!ws || q != 0) && (c != b'$' || q != 0 || !matches!(ch(i + 1), b'"' | b'\'')) {
                e = i;
            } else if e < s && buf.is_empty() {
                s = i + 1;
            }
        } else {
            flush(&mut buf, s, e);
            items.push(std::mem::take(&mut buf));
            (s, e) = (i + 1, i);
        }
        i += 1;
    }
    if s != 0 {
        flush(&mut buf, s, e);
        items.push(buf);
        items
    } else {
        vec![t[..(e + 1) as usize].to_vec()]
    }
}

/// CSP's section and key order (sub_181433790): digit runs compare as numbers.
fn natural(a: &[u8], b: &[u8]) -> i32 {
    let at = |s: &[u8], i: usize| s.get(i).map_or(0i32, |&c| i32::from(c as i8));
    let digit = |c: i32| (c - 48) as u32 <= 9;
    let (mut i, mut j) = (0usize, 0usize);
    let mut num = false;
    let mut c1 = at(a, 0);
    if c1 == 0 {
        return if at(b, 0) != 0 { -1 } else { 0 };
    }
    loop {
        let mut c2 = at(b, j);
        if c2 == 0 {
            break;
        }
        if num {
            let mut n1: i32 = 0;
            while digit(c1) {
                n1 = n1.wrapping_mul(10).wrapping_add(c1 - 48);
                i += 1;
                c1 = at(a, i);
                if c1 == 0 {
                    break;
                }
            }
            let mut n2: i32 = 0;
            while digit(c2) {
                n2 = n2.wrapping_mul(10).wrapping_add(c2 - 48);
                j += 1;
                c2 = at(b, j);
                if c2 == 0 {
                    break;
                }
            }
            let d = n1.wrapping_sub(n2);
            if d != 0 {
                return d;
            }
            num = false;
        } else {
            loop {
                if digit(c1) {
                    if !digit(c2) {
                        return -1;
                    }
                    num = true;
                    break;
                }
                if digit(c2) {
                    return 1;
                }
                if c1 != c2 {
                    return c1 - c2;
                }
                i += 1;
                j += 1;
                c1 = at(a, i);
                if c1 == 0 {
                    break;
                }
                c2 = at(b, j);
                if c2 == 0 {
                    break;
                }
            }
        }
        c1 = at(a, i);
        if c1 == 0 {
            return if at(b, j) != 0 { -1 } else { 0 };
        }
    }
    i32::from(at(a, i) != 0)
}

/// The flattened text (sub_18149CEB0, options: quote values, no spaces).
fn write(sections: &[Section]) -> B {
    let order = |a: &[u8], b: &[u8]| natural(a, b).cmp(&0);
    let mut secs: Vec<&Section> = sections.iter().collect();
    secs.sort_by(|a, b| a.name.cmp(&b.name));
    secs.sort_by(|a, b| order(&a.name, &b.name));
    let mut out = Vec::new();
    let keys = |out: &mut B, s: &Section| {
        let mut ks: Vec<&(B, Vec<B>)> = s.keys.iter().collect();
        ks.sort_by(|a, b| a.0.cmp(&b.0));
        ks.sort_by(|a, b| order(&a.0, &b.0));
        for (k, v) in ks {
            out.extend_from_slice(k);
            out.push(b'=');
            for (n, it) in v.iter().enumerate() {
                if n > 0 {
                    out.push(b',');
                }
                if !it.is_empty() && !it.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')) {
                    out.push(b'\'');
                    for &c in it {
                        if c == b'\'' {
                            out.push(b'\\');
                        }
                        out.push(c);
                    }
                    out.push(b'\'');
                } else {
                    out.extend_from_slice(it);
                }
            }
            out.push(b'\n');
        }
    };
    if let Some(s) = secs.iter().find(|s| s.name.is_empty()) {
        keys(&mut out, s);
        out.push(b'\n');
    }
    for s in secs.iter().filter(|s| !s.name.is_empty()) {
        out.push(b'[');
        out.extend_from_slice(&s.name);
        out.extend_from_slice(b"]\n");
        keys(&mut out, s);
        out.push(b'\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(t: &str) -> Vec<String> {
        split(t.as_bytes(), false).into_iter().map(|v| String::from_utf8(v).unwrap()).collect()
    }

    #[test]
    fn splits_like_csp() {
        // CSP's own results (C6a-2a probe t5)
        assert_eq!(items("1,\"2,3\",'4'"), ["1", "2,3", "4"]);
        assert_eq!(items("a\\,b,c"), ["a,b", "c"]);
        assert_eq!(items("\"ab\"cd"), ["\"ab\"cd"]);
        assert_eq!(items("x\"y\""), ["x\"y\""]);
        assert_eq!(items(", ,"), ["", "", ""]);
        assert_eq!(items("spaced  ,  out"), ["spaced", "out"]);
        assert_eq!(items("\"x\\ny\\tz\\q\""), ["x\ny\tz\\q"]);
    }

    #[test]
    fn generator_like_csp() {
        // CSP's own result for this file (C6a-2c probe q/gen5.ini, Lua off and on)
        let ini = "[TEMPLATE: _P]\nP = $1\n[TEMPLATE: _G1 EXTENDS _P]\n@OUTPUT = OUT_...\nA = ${Host:?}\nF = ${Flag:?}\nN = $Name\n\
                   [OUT_...]\nHost = h\nX = 1\n@GENERATOR = _G1, 2, Flag, Name = $Host\n[TEMPLATE: _G2]\nK = $1\n[]\n@GENERATOR = _G2, 2\n";
        let path = std::env::temp_dir().join(format!("rustyac_inipp_gen_{}.ini", std::process::id()));
        std::fs::write(&path, ini).unwrap();
        let out = flatten(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(out.unwrap(), "K=2\n\n[OUT_0]\nA=h\nF=1\nN=h\nP=1\n\n[OUT_1]\nA=h\nF=1\nN=h\nP=2\n\n[OUT_2]\nHost=h\nX=1\n\n");
    }

    #[test]
    fn natural_order() {
        assert!(natural(b"PROP_2", b"PROP_10") < 0);
        assert!(natural(b"A", b"B") < 0);
        assert!(natural(b"A", b"A_0") < 0);
        assert_eq!(natural(b"X1", b"X1"), 0);
    }

    #[test]
    fn lua_literals_like_csp() {
        // sub_18148DA30 / sub_18148C1C0, checked against CSP with q/l1.ini (C6a-2b)
        for (v, n) in [(&b""[..], true), (b" 1.5\t", true), (b"0x12", true), (b"0x1F", false), (b"1e+5", true), (b"1e", false), (b"1.2.3", false), (b"a", false), (b"1 2", false), (b"1\n", false)] {
            assert_eq!(csp_number(v), n, "{}", String::from_utf8_lossy(v));
        }
        let v = |s: &[&[u8]]| s.iter().map(|x| x.to_vec()).collect::<Vec<B>>();
        assert_eq!(literal(0, &[]), b"nil");
        assert_eq!(literal(0, &v(&[b"abc"])), br#""abc""#);
        assert_eq!(literal(0, &v(&[b"5"])), b"5");
        assert_eq!(literal(3, &v(&[b"1"])), b"true");
        assert_eq!(literal(13, &v(&[b"1"])), br#""1""#);
        assert_eq!(literal(0, &v(&[b"1", b"2", b"3"])), b"vec3(1,2,3)");
        assert_eq!(literal(0, &v(&[b"a", b"2"])), br#"{"a",2}"#);
        assert_eq!(quote(b"a\"b\\n"), br#""a\"b\\n""#);
    }
}
