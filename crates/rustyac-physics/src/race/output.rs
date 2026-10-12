// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `JsonOutputFile` (JsonOutputFile.obj) and the `JSONWriter` it uses: `out/race_out.json`,
//! byte for byte (key order, tabs, the line break before every `{`, `, ` between array
//! elements, numbers as a default C++ stream prints them, `\r\n` line ends, no last line end).

use super::manager::{OnSessionEndEvent, SESSION_RACE};

/// A node of the game's `JSONWriter`.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// keys in the order they were added
    Object(Vec<(String, Node)>),
    Array(Vec<Node>),
    /// a number, already as text
    Number(String),
    /// a string, written between quotes as it is (the callers escape)
    Text(String),
}

impl Node {
    pub fn int(value: i64) -> Node {
        Node::Number(value.to_string())
    }

    /// `Value(float)` @ 0x14023cbb0: `std::wostringstream << float`.
    pub fn float(value: f32) -> Node {
        Node::Number(stream_float(value))
    }

    /// A string through `ksEscapeJsonDoubleQuotesAndBackslash` @ 0x140239030.
    pub fn escaped(value: &str) -> Node {
        let mut out = String::with_capacity(value.len());
        for c in value.chars() {
            if c == '"' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
        Node::Text(out)
    }
}

/// A float as Visual C++ 2013's stream prints it by default: `%g` with six digits, and three
/// digits of exponent.
pub fn stream_float(value: f32) -> String {
    let v = value as f64;
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".to_string() } else { "0".to_string() };
    }
    if v.is_nan() {
        return if v.is_sign_negative() { "-1.#IND".to_string() } else { "1.#QNAN".to_string() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-1.#INF".to_string() } else { "1.#INF".to_string() };
    }
    // six significant digits, rounded as printf does (from the exact value)
    let scientific = format!("{:.5e}", v);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{}{:03}", trim(mantissa), sign, exponent.abs())
    } else {
        let decimals = (5 - exponent).max(0) as usize;
        trim(&format!("{:.*}", decimals, v))
    }
}

fn tabs(out: &mut String, count: usize) {
    for _ in 0..count {
        out.push('\t');
    }
}

/// `writeNode` @ 0x14023e160.
fn write_node(out: &mut String, node: &Node, indent: usize) {
    match node {
        // writeObject @ 0x14023e1f0
        Node::Object(children) => {
            out.push('\n');
            tabs(out, indent);
            out.push_str("{\n");
            for (i, (key, child)) in children.iter().enumerate() {
                if i != 0 {
                    out.push_str(",\n");
                }
                tabs(out, indent + 1);
                out.push('"');
                out.push_str(key);
                out.push_str("\": ");
                write_node(out, child, indent + 1);
            }
            out.push('\n');
            tabs(out, indent);
            out.push('}');
        }
        // writeArray @ 0x14023e080
        Node::Array(children) => {
            out.push('[');
            for (i, child) in children.iter().enumerate() {
                if i != 0 {
                    out.push_str(", ");
                }
                write_node(out, child, indent + 1);
            }
            out.push(']');
        }
        Node::Number(text) => out.push_str(text),
        Node::Text(text) => {
            out.push('"');
            out.push_str(text);
            out.push('"');
        }
    }
}

/// The file's bytes: UTF-8, written in text mode (`\n` becomes `\r\n`).
pub fn write(root: &Node) -> Vec<u8> {
    let mut text = String::new();
    write_node(&mut text, root, 0);
    text.replace('\n', "\r\n").into_bytes()
}

/// One car of the `players` list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    /// `driverInfo.name`
    pub name: String,
    /// `unixName`
    pub car: String,
    /// the skin folder's name
    pub skin: String,
}

/// `JsonOutputFile`.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonOutputFile {
    /// the track's name, with `-<layout>` when it has one
    pub track: String,
    pub number_of_sessions: i32,
    sessions: Vec<Node>,
    /// `sessionCounter`: how often each session of the list has ended
    session_counter: Vec<i32>,
}

impl JsonOutputFile {
    /// The constructor @ 0x14010c0e0.
    pub fn new(track: &str, layout: &str, number_of_sessions: i32) -> JsonOutputFile {
        let track = if layout.is_empty() { track.to_string() } else { format!("{track}-{layout}") };
        JsonOutputFile { track, number_of_sessions, sessions: Vec::new(), session_counter: vec![0; number_of_sessions.max(0) as usize] }
    }

    /// `endSession` @ 0x14010d310 with `offlineEndSession` @ 0x14010d730 or
    /// `onlineEndSession` @ 0x14010e1b0 (`online_lap_counts`: `RaceManager::getLapCount` of
    /// every car, online only). `session_index`: `RaceManager::getCurrentSessionIndex()`.
    pub fn end_session(&mut self, session_index: usize, event: &OnSessionEndEvent, cars: usize, online_lap_counts: Option<&[u32]>) {
        let session = &event.current_session;
        if self.session_counter.len() <= session_index {
            self.session_counter.resize(session_index + 1, 0);
        }
        let mut object = vec![
            ("event".to_string(), Node::int(self.session_counter[session_index] as i64)),
            ("name".to_string(), Node::escaped(&session.name)),
            ("type".to_string(), Node::int(session.session_type as i64)),
            ("lapsCount".to_string(), Node::int(session.laps as i64)),
            ("duration".to_string(), Node::float(session.duration_minutes)),
        ];
        let mut best: Vec<(u32, u32)> = vec![(0, u32::MAX); cars];
        let mut laps_total = vec![0i64; cars];
        let mut laps = Vec::new();
        for car in 0..cars {
            if event.result.laps.is_empty() {
                continue;
            }
            let Some(list) = event.result.laps.get(car) else {
                continue;
            };
            let n = list.len() as i32;
            let mut count = if session.laps <= n { session.laps } else { n };
            if count == 0 {
                count = n;
            }
            for (index, lap) in list.iter().enumerate().take(count.max(0) as usize) {
                let time = if !lap.is_valid && session.session_type != SESSION_RACE { -1 } else { lap.time as i32 as i64 };
                laps.push(Node::Object(vec![
                    ("lap".to_string(), Node::int(index as i64)),
                    ("car".to_string(), Node::int(car as i64)),
                    ("sectors".to_string(), Node::Array(lap.splits.iter().map(|s| Node::int(*s as i32 as i64)).collect())),
                    ("time".to_string(), Node::int(time)),
                    ("cuts".to_string(), Node::int(lap.cuts as i32 as i64)),
                    ("tyre".to_string(), Node::escaped(&lap.compound)),
                ]));
                if lap.time < best[car].1 && lap.time != 0 && lap.is_valid {
                    best[car] = (index as u32, lap.time);
                }
                laps_total[car] += 1;
            }
        }
        object.push(("laps".to_string(), Node::Array(laps)));
        let mut best_laps = Vec::new();
        match online_lap_counts {
            None => {
                object.push(("lapstotal".to_string(), Node::Array(laps_total.iter().map(|n| Node::int(*n)).collect())));
                for (car, (lap, time)) in best.iter().enumerate() {
                    if *time != u32::MAX {
                        best_laps.push(Node::Object(vec![("car".to_string(), Node::int(car as i64)), ("time".to_string(), Node::int(*time as i32 as i64)), ("lap".to_string(), Node::int(*lap as i32 as i64))]));
                    }
                }
            }
            Some(counts) => {
                object.push(("lapstotal".to_string(), Node::Array((0..cars).map(|car| Node::int(counts.get(car).copied().unwrap_or(0) as i32 as i64)).collect())));
                for (car, (lap, time)) in best.iter().enumerate() {
                    let time = *time as i32;
                    if 0 < time {
                        best_laps.push(Node::Object(vec![("car".to_string(), Node::int(car as i64)), ("time".to_string(), Node::int(time as i64)), ("lap".to_string(), Node::int(*lap as i32 as i64))]));
                    }
                }
            }
        }
        object.push(("bestLaps".to_string(), Node::Array(best_laps)));
        if session.session_type == SESSION_RACE {
            object.push(("raceResult".to_string(), Node::Array(event.result.positions.iter().map(|p| Node::int(*p as i64)).collect())));
        }
        self.session_counter[session_index] += 1;
        self.sessions.push(Node::Object(object));
    }

    /// `writeOutputFile` @ 0x14010ecc0 (with `writePlayers` @ 0x14010edb0 and
    /// `addExtraSessionInfo` @ 0x14010c5e0, whose drift / drag / time-attack / special-event
    /// blocks belong to modes that are not ported): the file's bytes. `instance_best_lap`:
    /// `RaceManager::getInstanceBestLap` of car 0.
    pub fn file(&self, players: &[Player], instance_best_lap: i32) -> Vec<u8> {
        let players = players.iter().map(|p| Node::Object(vec![("name".to_string(), Node::escaped(&p.name)), ("car".to_string(), Node::escaped(&p.car)), ("skin".to_string(), Node::escaped(&p.skin))])).collect();
        let extras = vec![Node::Object(vec![("name".to_string(), Node::Text("bestlap".to_string())), ("time".to_string(), Node::int(instance_best_lap as i64))])];
        let root = Node::Object(vec![
            ("track".to_string(), Node::Text(self.track.clone())),
            ("number_of_sessions".to_string(), Node::int(self.number_of_sessions as i64)),
            ("players".to_string(), Node::Array(players)),
            ("sessions".to_string(), Node::Array(self.sessions.clone())),
            ("extras".to_string(), Node::Array(extras)),
        ]);
        write(&root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::manager::{Session, SessionResult};

    /// The layout of a file the game itself wrote (a practice session without a lap).
    #[test]
    fn a_practice_without_laps_is_the_games_file() {
        let mut file = JsonOutputFile::new("some_circuit", "grandprix", 1);
        let event = OnSessionEndEvent { current_session: Session { session_type: 1, duration_minutes: 0.0, name: "Practice".to_string(), ..Session::default() }, result: SessionResult { laps: vec![Vec::new()], ..SessionResult::default() } };
        file.end_session(0, &event, 1, None);
        let bytes = file.file(&[Player { name: "driver".to_string(), car: "some_car".to_string(), skin: "99".to_string() }], 0);
        let expected = "\r\n{\r\n\t\"track\": \"some_circuit-grandprix\",\r\n\t\"number_of_sessions\": 1,\r\n\t\"players\": [\r\n\t\t{\r\n\t\t\t\"name\": \"driver\",\r\n\t\t\t\"car\": \"some_car\",\r\n\t\t\t\"skin\": \"99\"\r\n\t\t}],\r\n\t\"sessions\": [\r\n\t\t{\r\n\t\t\t\"event\": 0,\r\n\t\t\t\"name\": \"Practice\",\r\n\t\t\t\"type\": 1,\r\n\t\t\t\"lapsCount\": 0,\r\n\t\t\t\"duration\": 0,\r\n\t\t\t\"laps\": [],\r\n\t\t\t\"lapstotal\": [0],\r\n\t\t\t\"bestLaps\": []\r\n\t\t}],\r\n\t\"extras\": [\r\n\t\t{\r\n\t\t\t\"name\": \"bestlap\",\r\n\t\t\t\"time\": 0\r\n\t\t}]\r\n}";
        assert_eq!(String::from_utf8(bytes).unwrap(), expected);
    }

    #[test]
    fn floats_print_as_the_runtime_prints_them() {
        assert_eq!(stream_float(15.0), "15");
        assert_eq!(stream_float(7.5), "7.5");
        assert_eq!(stream_float(100000.0), "100000");
        assert_eq!(stream_float(1234567.0), "1.23457e+006");
        assert_eq!(stream_float(7.5548e-5), "7.5548e-005");
        assert_eq!(stream_float(0.0), "0");
    }
}
