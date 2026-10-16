// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! The .NET pieces the bridge reads its requests with: `NameValueCollection` (as
//! `HttpValueCollection` makes it), `HttpUtility.ParseQueryString`, `HttpUtility.UrlDecode` and
//! `HttpUtility.HtmlEncode`, as .NET Framework 4 has them.

/// `NameValueCollection` with `HttpValueCollection`'s comparer (ordinal, ignoring case): names
/// in the order they first came, each with its values. A name can be `None` (`a&b` has no `=`).
#[derive(Clone, Debug, Default)]
pub struct Params {
    entries: Vec<(Option<String>, Vec<Option<String>>)>,
}

/// `StringComparer.OrdinalIgnoreCase`: compares the upper case of each UTF-16 unit.
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    let a: Vec<u16> = a.encode_utf16().map(upper_unit).collect();
    let b: Vec<u16> = b.encode_utf16().map(upper_unit).collect();
    a == b
}

fn upper_unit(u: u16) -> u16 {
    if (0xd800..0xe000).contains(&u) {
        return u;
    }
    let c = char::from_u32(u as u32).unwrap();
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(one), None) if (one as u32) < 0x10000 => one as u16,
        _ => u,
    }
}

impl Params {
    fn find(&self, name: Option<&str>) -> Option<usize> {
        self.entries.iter().position(|(n, _)| match (n, name) {
            (None, None) => true,
            (Some(a), Some(b)) => eq_ignore_case(a, b),
            _ => false,
        })
    }

    /// `Add(name, value)`.
    pub fn add(&mut self, name: Option<String>, value: Option<String>) {
        match self.find(name.as_deref()) {
            Some(i) => self.entries[i].1.push(value),
            None => self.entries.push((name, vec![value])),
        }
    }

    /// `Add(NameValueCollection)`: every value of every name of the other.
    pub fn add_all(&mut self, other: &Params) {
        for (name, values) in &other.entries {
            for value in values {
                self.add(name.clone(), value.clone());
            }
        }
    }

    /// `Count`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `this[name]` / `Get(name)`: the values joined by commas, `None` when the name is not there.
    pub fn get(&self, name: &str) -> Option<String> {
        self.find(Some(name)).and_then(|i| joined(&self.entries[i].1))
    }

    /// `this[name]` with a name that may be `None` (`Parameters[Keys[0]]`).
    pub fn get_opt(&self, name: Option<&str>) -> Option<String> {
        self.find(name).and_then(|i| joined(&self.entries[i].1))
    }

    /// `this[index]` / `Get(index)`. Throws `ArgumentOutOfRangeException` past the end.
    pub fn at(&self, index: usize) -> Result<Option<String>, crate::bridge::Thrown> {
        match self.entries.get(index) {
            Some((_, values)) => Ok(joined(values)),
            None => Err(crate::bridge::Thrown::ArgumentOutOfRange),
        }
    }

    /// `Keys[index]`.
    pub fn key(&self, index: usize) -> Result<Option<String>, crate::bridge::Thrown> {
        match self.entries.get(index) {
            Some((name, _)) => Ok(name.clone()),
            None => Err(crate::bridge::Thrown::ArgumentOutOfRange),
        }
    }

    /// The names in order (`foreach (string name in Params)`).
    pub fn keys(&self) -> impl Iterator<Item = Option<&str>> {
        self.entries.iter().map(|(n, _)| n.as_deref())
    }
}

// NameValueCollection.GetAsOneString
fn joined(values: &[Option<String>]) -> Option<String> {
    match values.len() {
        0 => None,
        1 => values[0].clone(),
        _ => {
            let mut s = values[0].clone().unwrap_or_default();
            for v in &values[1..] {
                s.push(',');
                s.push_str(v.as_deref().unwrap_or(""));
            }
            Some(s)
        }
    }
}

/// `HttpUtility.ParseQueryString(query)` (UTF-8): `HttpValueCollection.FillFromString`.
pub fn parse_query_string(query: &str) -> Params {
    let query = query.strip_prefix('?').unwrap_or(query);
    let mut params = Params::default();
    let s: Vec<char> = query.chars().collect();
    let l = s.len();
    let mut i = 0;
    while i < l {
        let si = i;
        let mut ti: Option<usize> = None;
        while i < l {
            let ch = s[i];
            if ch == '=' {
                if ti.is_none() {
                    ti = Some(i);
                }
            } else if ch == '&' {
                break;
            }
            i += 1;
        }
        let (name, value): (Option<String>, String) = match ti {
            Some(ti) => (Some(s[si..ti].iter().collect()), s[ti + 1..i].iter().collect()),
            None => (None, s[si..i].iter().collect()),
        };
        params.add(name.as_deref().map(url_decode), Some(url_decode(&value)));
        if i + 1 == l && s[i] == '&' {
            params.add(None, Some(String::new()));
        }
        i += 1;
    }
    params
}

fn hex(u: u16) -> Option<u16> {
    match u {
        0x30..=0x39 => Some(u - 0x30),
        0x41..=0x46 => Some(u - 0x41 + 10),
        0x61..=0x66 => Some(u - 0x61 + 10),
        _ => None,
    }
}

/// `HttpUtility.UrlDecode(value)` (UTF-8): `+` is a space, `%XX` a byte, `%uXXXX` a UTF-16 unit.
pub fn url_decode(value: &str) -> String {
    let s: Vec<u16> = value.encode_utf16().collect();
    let count = s.len();
    let mut out: Vec<u16> = Vec::with_capacity(count);
    let mut bytes: Vec<u8> = Vec::new();
    let flush = |bytes: &mut Vec<u8>, out: &mut Vec<u16>| {
        if !bytes.is_empty() {
            out.extend(String::from_utf8_lossy(bytes).encode_utf16());
            bytes.clear();
        }
    };
    let mut pos = 0;
    while pos < count {
        let mut ch = s[pos];
        if ch == b'+' as u16 {
            ch = b' ' as u16;
        } else if ch == b'%' as u16 && pos + 2 < count {
            if s[pos + 1] == b'u' as u16 && pos + 5 < count {
                if let (Some(a), Some(b), Some(c), Some(d)) = (hex(s[pos + 2]), hex(s[pos + 3]), hex(s[pos + 4]), hex(s[pos + 5])) {
                    flush(&mut bytes, &mut out);
                    out.push((a << 12) | (b << 8) | (c << 4) | d);
                    pos += 6;
                    continue;
                }
            } else if let (Some(a), Some(b)) = (hex(s[pos + 1]), hex(s[pos + 2])) {
                bytes.push(((a << 4) | b) as u8);
                pos += 3;
                continue;
            }
        }
        if ch & 0xff80 == 0 {
            bytes.push(ch as u8);
        } else {
            flush(&mut bytes, &mut out);
            out.push(ch);
        }
        pos += 1;
    }
    flush(&mut bytes, &mut out);
    String::from_utf16_lossy(&out)
}

/// `HttpUtility.HtmlEncode(value)`.
pub fn html_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '&' => out.push_str("&amp;"),
            '\u{a0}'..='\u{ff}' => out.push_str(&format!("&#{};", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_like_dotnet() {
        let p = parse_query_string("?RACE/MODEL=abc%20d+e&x&y=1&y=2&");
        assert_eq!(p.key(0).unwrap().as_deref(), Some("RACE/MODEL"));
        assert_eq!(p.get("race/model").as_deref(), Some("abc d e"));
        assert_eq!(p.get_opt(None).as_deref(), Some("x,"));
        assert_eq!(p.get("y").as_deref(), Some("1,2"));
        assert_eq!(url_decode("%u00e8%C3%A8%zz%"), "èè%zz%");
    }
}
