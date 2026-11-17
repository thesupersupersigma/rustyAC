// SPDX-License-Identifier: MIT OR Apache-2.0

//! A progress screen for `tools/proofs/run_all.sh`. It only reads what a run writes under
//! `target/proofs` (`run.status`, `rows/`, `<area>/cases.log`, `times.tsv`, `now`, `last_case`, and
//! the last run of the mode in `cases/<mode>/`) and writes nothing: a run is the same with it or without.
//!
//!     proof_tui [<proofs dir, default $PROOF_OUT or target/proofs>] [--plain]
//!
//! Without a console of its own (Git Bash's mintty, a pipe) or with `--plain` it prints a line of
//! text every 10 s instead, and each DIFFERENT / ERROR as it comes, and stops at the end of the run.

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, Paragraph};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERDICTS: [&str; 5] = ["IDENTICAL", "DIFFERENT", "ERROR", "SKIPPED", "EXPECTED"];
const GB: f64 = 1073741824.0;
// red below these, as tools/proofs wants them (docs/port/proofs.md: the 18 GB keep floor, 12 GB = fewer lanes)
const DISK_FLOOR: f64 = 18.0;
const RAM_FLOOR: f64 = 12.0;

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}
fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}
fn mtime(p: &Path) -> Option<f64> {
    Some(fs::metadata(p).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}
/// times.tsv: `<secs>\t<case>`, one row per verdict
fn times(p: &Path) -> Vec<(String, f64)> {
    read(p)
        .lines()
        .filter_map(|l| {
            let (s, c) = l.split_once('\t')?;
            Some((c.trim().to_string(), s.trim().parse().ok()?))
        })
        .collect()
}
fn hm(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, s % 3600 / 60)
    } else {
        format!("{}m{:02}s", s / 60, s % 60)
    }
}

#[derive(Default)]
struct Area {
    name: String,
    /// the cases of the last run of this mode (more if this run has more)
    total: Option<usize>,
    counts: [usize; 5],
    started: bool,
    done: bool,
    secs: f64,
    /// the key of the case running now and the seconds since the case before ended
    now: Option<(String, f64)>,
    /// seconds still to go, from the last run's times scaled by how this run compares so far
    left: Option<f64>,
}
impl Area {
    fn cases(&self) -> usize {
        self.counts.iter().sum()
    }
}

struct Run {
    mode: String,
    elapsed: f64,
    end: bool,
    lanes: Vec<Vec<Area>>,
    /// every DIFFERENT / ERROR line so far, with the area's log
    bad: Vec<String>,
}
impl Run {
    fn cases(&self) -> (usize, usize) {
        let a = self.lanes.iter().flatten();
        (a.clone().map(Area::cases).sum(), a.map(|a| a.total.unwrap_or(0).max(a.cases())).sum())
    }
    /// the longest lane's time left (None until some lane has a guess)
    fn left(&self) -> Option<f64> {
        let lane = |l: &Vec<Area>| l.iter().map(|a| a.left).sum::<Option<f64>>();
        self.lanes.iter().filter_map(lane).reduce(f64::max)
    }
    fn lanes_done(&self) -> bool {
        self.lanes.iter().flatten().all(|a| a.done)
    }
}

fn load_area(dir: &Path, mode: &str, start: f64, t: f64, name: String, bad: &mut Vec<String>) -> Area {
    let d = dir.join(&name);
    let kept = dir.join("cases").join(mode);
    let last = times(&kept.join(format!("{name}.times")));
    let mut a = Area { total: Some(read(&kept.join(format!("{name}.txt"))).lines().count()).filter(|&n| n > 0), ..Default::default() };
    // lib.sh writes quick.re when the area starts (after deleting the area's folder)
    let began = mtime(&d.join("quick.re")).filter(|&m| m >= start - 1.0);
    a.started = began.is_some();
    a.done = a.started && dir.join("rows").join(format!("{name}.row")).exists();
    let cur = if a.started { times(&d.join("times.tsv")) } else { vec![] };
    if let Some(began) = began {
        for l in read(&d.join("cases.log")).lines() {
            if let Some(i) = VERDICTS.iter().position(|v| l.starts_with(v) && l[v.len()..].starts_with(' ')) {
                a.counts[i] += 1;
                if i == 1 || i == 2 {
                    bad.push(format!("{name} {l}   log: {}", dir.join(format!("{name}.log")).display()));
                }
            }
        }
        a.secs = if a.done { read(&dir.join("rows").join(format!("{name}.secs"))).trim().parse().unwrap_or(t - began) } else { t - began };
        if !a.done {
            let since = read(&d.join("last_case")).trim().parse::<f64>().map_or(0.0, |c| t - c);
            let now = read(&d.join("now")).trim().to_string();
            // areas that never call keep() (shaders) have no "now" file
            a.now = Some((if now.is_empty() { "?".into() } else { now }, since));
        }
    }
    let since = a.now.as_ref().map_or(0.0, |n| n.1);
    let sum_last: f64 = last.iter().map(|x| x.1).sum();
    a.left = if a.done {
        Some(0.0)
    } else if sum_last > 0.0 {
        let (mut act, mut was) = (0.0, 0.0);
        for (c, s) in &cur {
            if let Some((_, l)) = last.iter().find(|x| &x.0 == c) {
                act += s;
                was += l;
            }
        }
        let f = if was > 0.0 { (act / was).clamp(0.25, 4.0) } else { 1.0 };
        Some(((sum_last - was) * f - since).max(0.0))
    } else if let (Some(tot), n @ 1..) = (a.total, a.cases()) {
        let act: f64 = cur.iter().map(|x| x.1).sum();
        Some((act / n as f64 * tot.saturating_sub(n) as f64 - since).max(0.0))
    } else {
        None
    };
    a.name = name;
    a
}

/// None while there is no run.status (none yet, or run_all.sh is still building)
fn load(dir: &Path) -> Option<Run> {
    let (mut mode, mut start, mut end, mut lanes) = (String::new(), None, None, vec![]);
    for l in read(&dir.join("run.status")).lines() {
        match l.split_once(' ') {
            Some(("mode", m)) => mode = m.trim().to_string(),
            Some(("start", s)) => start = s.trim().parse::<f64>().ok(),
            Some(("end", s)) => end = s.trim().parse::<f64>().ok(),
            Some(("lane", a)) => lanes.push(a.split_whitespace().map(str::to_string).collect::<Vec<_>>()),
            _ => {}
        }
    }
    let start = start?;
    let t = end.unwrap_or_else(now);
    let mut bad = vec![];
    let lanes = lanes.into_iter().map(|l| l.into_iter().map(|n| load_area(dir, &mode, start, t, n, &mut bad)).collect()).collect();
    Some(Run { mode, elapsed: t - start, end: end.is_some(), lanes, bad })
}

/// what run_all.sh prints at the end: table.md, then the ERROR, skipped and vanished cases
fn summary(dir: &Path, mode: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = read(&dir.join("table.md")).lines().map(|l| (l.to_string(), false)).collect();
    for (file, what) in [("errors.txt", "cases ERROR".to_string()), ("skipped.txt", "cases skipped".into()), ("vanished.txt", format!("cases vanished since the last {mode} run"))] {
        let p = dir.join(file);
        let lines: Vec<String> = read(&p).lines().map(|l| format!("  {}", l.strip_prefix("SKIPPED ").unwrap_or(l))).collect();
        if lines.is_empty() {
            out.push((format!("0 {what}"), false));
        } else {
            out.push((format!("{} {what} ({}):", lines.len(), p.display()), true));
            out.extend(lines.into_iter().map(|l| (l, false)));
        }
    }
    out
}

fn free_gb() -> (f64, f64) {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let root: Vec<u16> = "C:\\\0".encode_utf16().collect();
    let mut disk = 0u64;
    // SAFETY: plain out-parameters of the right size
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    unsafe {
        GetDiskFreeSpaceExW(root.as_ptr(), &mut disk, std::ptr::null_mut(), std::ptr::null_mut());
        GlobalMemoryStatusEx(&mut m);
    }
    (disk as f64 / GB, m.ullAvailPhys as f64 / GB)
}

/// whether stdout is a real console (Windows Terminal, conhost, winpty), not mintty's pipe
fn console() -> bool {
    use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE};
    let mut m = 0;
    // SAFETY: GetConsoleMode only fails on a handle that is not a console
    unsafe { GetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), &mut m) != 0 }
}

fn bar(w: usize, f: f64) -> String {
    let n = ((f.clamp(0.0, 1.0) * w as f64).round() as usize).min(w);
    "█".repeat(n) + &"░".repeat(w - n)
}

fn lane_lines(lane: &[Area], w: usize) -> Vec<Line<'static>> {
    let mut out = vec![];
    for a in lane {
        let n = a.cases();
        let tot = a.total.map_or("?".to_string(), |t| t.max(n).to_string());
        let red = a.counts[1] + a.counts[2] > 0;
        let colour = if red { Color::Red } else if a.done { Color::Green } else if a.started { Color::Yellow } else { Color::DarkGray };
        out.push(Line::from(vec![Span::styled(a.name.clone(), Style::new().fg(colour).bold()), Span::raw(format!("  {n}/{tot}"))]));
        let f = a.total.map_or(if a.done { 1.0 } else { 0.0 }, |t| n as f64 / t.max(n).max(1) as f64);
        out.push(Line::styled(bar(w, if a.done { 1.0 } else { f }), Style::new().fg(colour)));
        let c = &a.counts;
        out.push(Line::from(vec![
            Span::raw(format!("{} identical ", c[0])),
            Span::styled(format!("{} diff ", c[1]), if c[1] > 0 { Style::new().fg(Color::Red).bold() } else { Style::new() }),
            Span::styled(format!("{} err ", c[2]), if c[2] > 0 { Style::new().fg(Color::Red).bold() } else { Style::new() }),
            Span::raw(format!("{} skip", c[3] + c[4])),
        ]));
        let left = a.left.map_or("?".to_string(), hm);
        out.push(Line::raw(if a.done {
            format!("done in {}", hm(a.secs))
        } else if a.started {
            format!("{} so far, ~{left} left", hm(a.secs))
        } else {
            format!("waiting, ~{left}")
        }));
        if let Some((k, s)) = &a.now {
            out.push(Line::styled(format!("now: {k} ({})", hm(*s)), Style::new().fg(Color::Cyan)));
        }
        out.push(Line::raw(""));
    }
    out
}

fn draw(f: &mut ratatui::Frame, dir: &Path, run: &Option<Run>) {
    let (disk, ram) = free_gb();
    let mut head = vec![Span::styled("rustyAC proofs ", Style::new().bold())];
    if let Some(r) = run {
        let left = match (r.end, r.left()) {
            (true, _) => "run done".to_string(),
            (_, Some(l)) if r.lanes_done() => format!("~{} left, ERROR cases again, alone", hm(l)),
            (_, Some(l)) => format!("~{} left", hm(l)),
            _ => "time left: ?".into(),
        };
        head.push(Span::raw(format!("{}   {}   {left}   ", r.mode, hm(r.elapsed))));
    } else {
        head.push(Span::raw(format!("waiting for run_all.sh to start (or end its builds) in {}   ", dir.display())));
    }
    let low = |v: f64, floor: f64| if v < floor { Style::new().fg(Color::Red).bold() } else { Style::new() };
    head.push(Span::styled(format!("C: {disk:.1} GB free"), low(disk, DISK_FLOOR)));
    head.push(Span::raw("   "));
    head.push(Span::styled(format!("RAM {ram:.1} GB free"), low(ram, RAM_FLOOR)));
    if run.is_some() {
        head.push(Span::raw(format!("   {}", dir.display())));
    }
    let bad = run.as_ref().map_or(0, |r| r.bad.len());
    let [h, g, body, b, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(if bad > 0 { (bad as u16 + 2).min(10) } else { 0 }),
        Constraint::Length(1),
    ])
    .areas(f.area());
    f.render_widget(Paragraph::new(Line::from(head)), h);
    // the PC's proof lock (tools/proofs/lock.sh): which checkout's run has it, a waiting run's answer
    let lock = read(&Path::new(&std::env::var("USERPROFILE").unwrap_or_default()).join(".rustyac/proof.lock/owner"));
    let owner = match lock.lines().collect::<Vec<_>>()[..] {
        [repo, pid, start, ..] => format!("proof lock: {repo} (PID {pid}, started {start})"),
        _ => "proof lock: free".into(),
    };
    f.render_widget(Line::styled(format!("watches target/proofs only · q quits · {owner}"), Style::new().fg(Color::DarkGray)), foot);
    let Some(r) = run else { return };
    let (n, tot) = r.cases();
    let red = !r.bad.is_empty();
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::new().fg(if red { Color::Red } else { Color::Green }))
            .ratio(if r.end { 1.0 } else { n as f64 / tot.max(1) as f64 }.min(1.0))
            .label(format!("{n}/{tot} cases")),
        g,
    );
    if r.end {
        let lines: Vec<Line> = summary(dir, &r.mode).into_iter().map(|(l, red)| if red { Line::styled(l, Style::new().fg(Color::Red)) } else { Line::raw(l) }).collect();
        f.render_widget(Paragraph::new(lines).block(Block::bordered().title(" the run's table (table.md) ")), body);
    } else {
        let cols = Layout::horizontal(vec![Constraint::Ratio(1, r.lanes.len().max(1) as u32); r.lanes.len()]).split(body);
        for (i, (lane, c)) in r.lanes.iter().zip(cols.iter()).enumerate() {
            let w = c.width.saturating_sub(2) as usize;
            f.render_widget(Paragraph::new(lane_lines(lane, w)).block(Block::bordered().title(format!(" lane {} ", i + 1))), *c);
        }
    }
    if red {
        let lines: Vec<Line> = r.bad.iter().map(|l| Line::styled(l.clone(), Style::new().fg(Color::Red))).collect();
        let skip = lines.len().saturating_sub(b.height.saturating_sub(2) as usize);
        f.render_widget(Paragraph::new(lines[skip..].to_vec()).block(Block::bordered().title(" DIFFERENT / ERROR ").red()), b);
    }
}

fn plain(dir: &Path) {
    let (mut seen, mut next, mut waited) = (HashSet::new(), 0.0, false);
    loop {
        match load(dir) {
            None if !waited => {
                println!("waiting for run_all.sh to start in {}", dir.display());
                waited = true;
            }
            None => {}
            Some(r) => {
                for l in &r.bad {
                    if seen.insert(l.clone()) {
                        println!("\x1b[31m{l}\x1b[0m");
                    }
                }
                if r.end {
                    for (l, red) in summary(dir, &r.mode) {
                        println!("{}", if red { format!("\x1b[31m{l}\x1b[0m") } else { l });
                    }
                    return;
                }
                if now() >= next {
                    next = now() + 10.0;
                    let (n, tot) = r.cases();
                    let (disk, ram) = free_gb();
                    let lanes: Vec<String> = r
                        .lanes
                        .iter()
                        .map(|l| l.iter().find(|a| !a.done).map_or("done".into(), |a| format!("{} {}/{}", a.name, a.cases(), a.total.map_or("?".into(), |t| t.to_string()))))
                        .collect();
                    let left = r.left().map_or("?".into(), hm);
                    println!("{} {n}/{tot} cases, ~{left} left | {} | C: {disk:.1} GB, RAM {ram:.1} GB free", hm(r.elapsed), lanes.join(" | "));
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from).unwrap_or_else(|| std::env::var_os("PROOF_OUT").map_or("target/proofs".into(), PathBuf::from));
    if !dir.is_dir() {
        eprintln!("proof_tui: no folder {} (give run_all.sh's target/proofs)", dir.display());
        std::process::exit(2);
    }
    if args.iter().any(|a| a == "--plain") || !console() {
        return plain(&dir);
    }
    let mut term = ratatui::init();
    loop {
        let run = load(&dir);
        if let Err(e) = term.draw(|f| draw(f, &dir, &run)) {
            ratatui::restore();
            eprintln!("proof_tui: {e}");
            std::process::exit(1);
        }
        if event::poll(Duration::from_secs(1)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                let ctrl_c = k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL);
                if k.kind == KeyEventKind::Press && (matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) || ctrl_c) {
                    break;
                }
            }
        }
    }
    ratatui::restore();
}
