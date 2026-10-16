// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! .NET Framework 4's `JavaScriptSerializer` as the launcher uses it: `Deserialize` into
//! dictionaries, lists and numbers (`JavaScriptObjectDeserializer`, which takes more than JSON:
//! single quotes, bare names, a later duplicate name replacing the earlier value in place) and
//! `Serialize` back (no spaces, `<`, `>`, `'` and `&` written as `\u00XX`).

/// What `Deserialize` makes of a value. Numbers keep the type .NET gives them: `int`, then
/// `long`, then `decimal` (which keeps its trailing zeros), then `double`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i32),
    Long(i64),
    /// `decimal`: sign, the digits without leading zeros and how many of them are decimals.
    Decimal(bool, u128, u32),
    Double(f64),
    Str(String),
    /// `Dictionary<string, object>`: the names in insertion order.
    Object(Vec<(String, Value)>),
    Array(Vec<Value>),
}

impl Value {
    pub fn obj() -> Value {
        Value::Object(Vec::new())
    }

    /// `dictionary[name] = value`: replaces in place or adds at the end.
    pub fn set(&mut self, name: &str, value: Value) {
        if let Value::Object(members) = self {
            match members.iter_mut().find(|(n, _)| n == name) {
                Some(m) => m.1 = value,
                None => members.push((name.to_string(), value)),
            }
        }
    }

    /// `dictionary.Add(name, value)`: false (the .NET ArgumentException) when it is there.
    pub fn add(&mut self, name: &str, value: Value) -> bool {
        if let Value::Object(members) = self {
            if members.iter().any(|(n, _)| n == name) {
                return false;
            }
            members.push((name.to_string(), value));
        }
        true
    }

    pub fn contains(&self, name: &str) -> bool {
        matches!(self, Value::Object(m) if m.iter().any(|(n, _)| n == name))
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }
}

const MAX_JSON_LENGTH: usize = 2_097_152;
const RECURSION_LIMIT: usize = 100;

/// `JavaScriptSerializer.DeserializeObject(input)`; `Err` stands for the exception it throws.
#[allow(clippy::result_unit_err)]
pub fn deserialize(input: &str) -> Result<Value, ()> {
    if input.encode_utf16().count() > MAX_JSON_LENGTH {
        return Err(());
    }
    let mut d = Deserializer { s: input.chars().collect(), i: 0 };
    let v = d.internal(0)?;
    if d.next_non_empty().is_some() {
        return Err(());
    }
    Ok(v)
}

/// `Deserialize<Dictionary<string, object>>(input)`: `Ok(None)` for `null`.
#[allow(clippy::result_unit_err)]
pub fn deserialize_dictionary(input: &str) -> Result<Option<Value>, ()> {
    match deserialize(input)? {
        Value::Null => Ok(None),
        v @ Value::Object(_) => Ok(Some(v)),
        _ => Err(()),
    }
}

struct Deserializer {
    s: Vec<char>,
    i: usize,
}

impl Deserializer {
    fn move_next(&mut self) -> Option<char> {
        let c = self.s.get(self.i).copied();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    fn next_non_empty(&mut self) -> Option<char> {
        while let Some(c) = self.move_next() {
            if !c.is_whitespace() {
                return Some(c);
            }
        }
        None
    }

    fn internal(&mut self, depth: usize) -> Result<Value, ()> {
        let depth = depth + 1;
        if depth > RECURSION_LIMIT {
            return Err(());
        }
        let Some(c) = self.next_non_empty() else { return Ok(Value::Null) };
        self.i -= 1;
        match c {
            '{' => self.dictionary(depth),
            '[' => self.list(depth),
            '"' | '\'' => self.string().map(Value::Str),
            _ => self.primitive(),
        }
    }

    fn dictionary(&mut self, depth: usize) -> Result<Value, ()> {
        let mut members: Option<Vec<(String, Value)>> = None;
        if self.move_next() != Some('{') {
            return Err(());
        }
        let mut c;
        loop {
            c = self.next_non_empty();
            if c.is_none() {
                break;
            }
            self.i -= 1;
            if c == Some(':') {
                return Err(());
            }
            let mut name: Option<String> = None;
            if c != Some('}') {
                name = self.member_name()?;
                c = self.next_non_empty();
                if c != Some(':') {
                    return Err(());
                }
            }
            if members.is_none() {
                members = Some(Vec::new());
                if name.as_deref().unwrap_or("").is_empty() {
                    c = self.next_non_empty();
                    break;
                }
            }
            let value = self.internal(depth)?;
            // dictionary[memberName] = value; a null name throws
            let name = name.ok_or(())?;
            let m = members.as_mut().unwrap();
            match m.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = value,
                None => m.push((name, value)),
            }
            c = self.next_non_empty();
            if c == Some('}') {
                break;
            }
            if c != Some(',') {
                return Err(());
            }
        }
        if c != Some('}') {
            return Err(());
        }
        Ok(Value::Object(members.unwrap_or_default()))
    }

    fn member_name(&mut self) -> Result<Option<String>, ()> {
        let Some(c) = self.next_non_empty() else { return Ok(None) };
        self.i -= 1;
        if c == '"' || c == '\'' {
            return self.string().map(Some);
        }
        Ok(Some(self.primitive_token()))
    }

    fn list(&mut self, depth: usize) -> Result<Value, ()> {
        let mut list = Vec::new();
        if self.move_next() != Some('[') {
            return Err(());
        }
        let mut expect_more = false;
        let mut c;
        loop {
            c = self.next_non_empty();
            if c.is_none() || c == Some(']') {
                break;
            }
            self.i -= 1;
            list.push(self.internal(depth)?);
            expect_more = false;
            c = self.next_non_empty();
            if c == Some(']') {
                break;
            }
            expect_more = true;
            if c != Some(',') {
                return Err(());
            }
        }
        if expect_more || c != Some(']') {
            return Err(());
        }
        Ok(Value::Array(list))
    }

    fn string(&mut self) -> Result<String, ()> {
        let mut sb = String::new();
        let mut escaped = false;
        let quote = self.move_next().ok_or(())?;
        if quote != '"' && quote != '\'' {
            return Err(());
        }
        let mut pending_high: Option<u16> = None;
        while let Some(c) = self.move_next() {
            if c == '\\' {
                if escaped {
                    sb.push('\\');
                    escaped = false;
                } else {
                    escaped = true;
                }
                continue;
            }
            if escaped {
                escaped = false;
                let e = match c {
                    '"' | '\'' | '/' => c,
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| self.move_next()).collect();
                        let unit = u16::from_str_radix(&hex, 16).map_err(|_| ())?;
                        // a \uXXXX pair makes one character
                        if (0xd800..0xdc00).contains(&unit) {
                            pending_high = Some(unit);
                            continue;
                        }
                        if let (Some(high), true) = (pending_high.take(), (0xdc00..0xe000).contains(&unit)) {
                            sb.extend(char::decode_utf16([high, unit]).map(|r| r.unwrap_or('\u{fffd}')));
                            continue;
                        }
                        char::from_u32(unit as u32).unwrap_or('\u{fffd}')
                    }
                    _ => return Err(()),
                };
                sb.push(e);
            } else {
                if c == quote {
                    return Ok(sb);
                }
                sb.push(c);
            }
            if pending_high.take().is_some() {
                sb.push('\u{fffd}');
            }
        }
        Err(())
    }

    fn primitive_token(&mut self) -> String {
        let mut sb = String::new();
        while let Some(c) = self.move_next() {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '+' {
                sb.push(c);
            } else {
                self.i -= 1;
                break;
            }
        }
        sb
    }

    fn primitive(&mut self) -> Result<Value, ()> {
        let input = self.primitive_token();
        match input.as_str() {
            "null" => return Ok(Value::Null),
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            _ => {}
        }
        let has_point = input.contains('.');
        if !input.to_ascii_lowercase().contains('e') {
            if !has_point {
                if let Some(n) = parse_integer(&input) {
                    if let Ok(n) = i32::try_from(n) {
                        return Ok(Value::Int(n));
                    }
                    if let Ok(n) = i64::try_from(n) {
                        return Ok(Value::Long(n));
                    }
                }
            }
            if let Some((neg, mantissa, scale)) = parse_decimal(&input) {
                return Ok(Value::Decimal(neg, mantissa, scale));
            }
        }
        parse_double(&input).map(Value::Double).ok_or(())
    }
}

// Int32/Int64.TryParse with NumberStyles.Integer (a leading sign, digits).
fn parse_integer(s: &str) -> Option<i128> {
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    if digits.len() > 30 {
        return None;
    }
    let n: i128 = if digits.is_empty() { 0 } else { digits.parse().ok()? };
    Some(if neg { -n } else { n })
}

// Decimal.TryParse with NumberStyles.Number: a sign in front or behind, one decimal point.
// The value is rounded to the 96 bits and 28 decimals a decimal holds.
fn parse_decimal(s: &str) -> Option<(bool, u128, u32)> {
    let mut t = s;
    let mut neg = false;
    if let Some(r) = t.strip_prefix('-') {
        neg = true;
        t = r;
    } else if let Some(r) = t.strip_prefix('+') {
        t = r;
    } else if let Some(r) = t.strip_suffix('-') {
        neg = true;
        t = r;
    } else if let Some(r) = t.strip_suffix('+') {
        t = r;
    }
    let (int, frac) = match t.find('.') {
        Some(p) => (&t[..p], &t[p + 1..]),
        None => (t, ""),
    };
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    if !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    let mut scale = frac.len() as i64;
    while digits.len() > 1 && digits[0] == 0 {
        digits.remove(0);
    }
    const MAX: u128 = (1u128 << 96) - 1;
    loop {
        if digits.len() <= 29 {
            let m = digits.iter().fold(0u128, |m, &d| m * 10 + d as u128);
            if m <= MAX && scale <= 28 {
                if m == 0 {
                    neg = false;
                }
                return Some((neg, m, scale as u32));
            }
        }
        if scale <= 0 {
            return None;
        }
        let last = digits.pop().unwrap();
        scale -= 1;
        if last >= 5 {
            let mut k = digits.len();
            loop {
                if k == 0 {
                    digits.insert(0, 1);
                    break;
                }
                k -= 1;
                if digits[k] == 9 {
                    digits[k] = 0;
                } else {
                    digits[k] += 1;
                    break;
                }
            }
        }
        if digits.is_empty() {
            digits.push(0);
        }
    }
}

// Double.TryParse with NumberStyles.Float and the invariant culture.
fn parse_double(s: &str) -> Option<f64> {
    match s {
        "NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

/// `JavaScriptSerializer.Serialize(value)`.
pub fn serialize(v: &Value) -> String {
    let mut sb = String::new();
    write(v, &mut sb);
    sb
}

fn write(v: &Value, sb: &mut String) {
    match v {
        Value::Null => sb.push_str("null"),
        Value::Bool(b) => sb.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => sb.push_str(&n.to_string()),
        Value::Long(n) => sb.push_str(&n.to_string()),
        Value::Decimal(neg, m, scale) => sb.push_str(&decimal_string(*neg, *m, *scale)),
        Value::Double(d) => sb.push_str(&double_r(*d)),
        Value::Str(s) => quote(s, sb),
        Value::Object(members) => {
            sb.push('{');
            let mut first = true;
            // "__type" goes first
            if let Some((n, v)) = members.iter().find(|(n, _)| n == "__type") {
                quote(n, sb);
                sb.push(':');
                write(v, sb);
                first = false;
            }
            for (n, v) in members {
                if n == "__type" {
                    continue;
                }
                if !first {
                    sb.push(',');
                }
                quote(n, sb);
                sb.push(':');
                write(v, sb);
                first = false;
            }
            sb.push('}');
        }
        Value::Array(items) => {
            sb.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    sb.push(',');
                }
                write(item, sb);
            }
            sb.push(']');
        }
    }
}

/// `JavaScriptString.QuoteString` inside double quotes.
pub fn quote(s: &str, sb: &mut String) {
    sb.push('"');
    for c in s.chars() {
        match c {
            '\r' => sb.push_str("\\r"),
            '\t' => sb.push_str("\\t"),
            '"' => sb.push_str("\\\""),
            '\\' => sb.push_str("\\\\"),
            '\n' => sb.push_str("\\n"),
            '\u{8}' => sb.push_str("\\b"),
            '\u{c}' => sb.push_str("\\f"),
            c if (c as u32) < 0x20 || matches!(c, '\'' | '<' | '>' | '&' | '\u{85}' | '\u{2028}' | '\u{2029}') => {
                sb.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => sb.push(c),
        }
    }
    sb.push('"');
}

// decimal.ToString(CultureInfo.InvariantCulture)
fn decimal_string(neg: bool, m: u128, scale: u32) -> String {
    let mut digits = m.to_string();
    let scale = scale as usize;
    if digits.len() <= scale {
        digits = "0".repeat(scale + 1 - digits.len()) + &digits;
    }
    let point = digits.len() - scale;
    let mut s = String::new();
    if neg && m != 0 {
        s.push('-');
    }
    s.push_str(&digits[..point]);
    if scale > 0 {
        s.push('.');
        s.push_str(&digits[point..]);
    }
    s
}

/// `double.ToString("r", CultureInfo.InvariantCulture)` as .NET Framework writes it: 15
/// digits when they read back as the same number, else 17, in the "G" layout.
pub fn double_r(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if d == 0.0 {
        return "0".into();
    }
    let mut precision = 15;
    let mut e = format!("{:.*e}", precision - 1, d);
    if e.parse::<f64>().ok() != Some(d) {
        precision = 17;
        e = format!("{:.*e}", precision - 1, d);
    }
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // the decimal point sits after `scale` digits
    let scale = exp + 1;
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    if scale > precision as i32 || scale < -3 {
        s.push_str(&digits[..1]);
        if digits.len() > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        let x = scale - 1;
        s.push_str(&format!("E{}{:02}", if x < 0 { '-' } else { '+' }, x.abs()));
    } else if scale <= 0 {
        s.push_str("0.");
        s.push_str(&"0".repeat((-scale) as usize));
        s.push_str(digits);
    } else {
        let scale = scale as usize;
        if digits.len() <= scale {
            s.push_str(digits);
            s.push_str(&"0".repeat(scale - digits.len()));
        } else {
            s.push_str(&digits[..scale]);
            s.push('.');
            s.push_str(&digits[scale..]);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_like_dotnet() {
        let v = deserialize("{ name : 'a<b>', \"n\": 1.50, \"big\": 12345678901, \"e\": 1e20, \"x\": 2, \"x\": [true,null] }").unwrap();
        assert_eq!(serialize(&v), "{\"name\":\"a\\u003cb\\u003e\",\"n\":1.50,\"big\":12345678901,\"e\":1E+20,\"x\":[true,null]}");
        assert!(deserialize("{\"a\":1,}").is_err());
        assert!(deserialize("[1,]").is_err());
        assert_eq!(double_r(1.5e-7), "1.5E-07");
        assert_eq!(double_r(0.1), "0.1");
        assert_eq!(double_r(123456.0), "123456");
    }
}
