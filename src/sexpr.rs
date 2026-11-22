//! Minimal S-expression parser / writer for KiCad files.
//! Quoted strings are kept raw (escapes untouched) so they round-trip exactly.

#[derive(Debug, Clone)]
pub enum Sexp {
    /// (text, was_quoted)
    Atom(String, bool),
    List(Vec<Sexp>),
}

impl Sexp {
    pub fn sym(s: &str) -> Sexp {
        Sexp::Atom(s.to_string(), false)
    }
    pub fn str(s: &str) -> Sexp {
        Sexp::Atom(escape(s), true)
    }
    pub fn num(v: f64) -> Sexp {
        Sexp::Atom(fmt_num(v), false)
    }
    pub fn list(v: Vec<Sexp>) -> Sexp {
        Sexp::List(v)
    }

    /// First atom of a list (the node name), e.g. "gr_line".
    pub fn name(&self) -> Option<&str> {
        match self {
            Sexp::List(v) => match v.first() {
                Some(Sexp::Atom(s, false)) => Some(s.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn children(&self) -> &[Sexp] {
        match self {
            Sexp::List(v) => v,
            _ => &[],
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Sexp>> {
        match self {
            Sexp::List(v) => Some(v),
            _ => None,
        }
    }

    /// First child list with the given name.
    pub fn child(&self, name: &str) -> Option<&Sexp> {
        self.children().iter().find(|c| c.name() == Some(name))
    }

    /// All child lists with the given name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.children().iter().filter(move |c| c.name() == Some(name))
    }

    /// Atom at index i (0 = name).
    pub fn atom(&self, i: usize) -> Option<&str> {
        match self.children().get(i) {
            Some(Sexp::Atom(s, _)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn is_quoted(&self, i: usize) -> bool {
        matches!(self.children().get(i), Some(Sexp::Atom(_, true)))
    }

    pub fn f64_at(&self, i: usize) -> Option<f64> {
        match self.children().get(i) {
            Some(Sexp::Atom(s, false)) => s.parse().ok(),
            _ => None,
        }
    }

    /// (name x y) -> (x, y)
    pub fn xy(&self, name: &str) -> Option<(f64, f64)> {
        let c = self.child(name)?;
        Some((c.f64_at(1)?, c.f64_at(2)?))
    }

    pub fn set(&mut self, i: usize, v: Sexp) {
        if let Sexp::List(l) = self {
            if i < l.len() {
                l[i] = v;
            } else {
                l.push(v);
            }
        }
    }

    pub fn write(&self, out: &mut String) {
        write_rec(self, out, 0);
        out.push('\n');
    }
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            _ => o.push(c),
        }
    }
    o
}

pub fn fmt_num(v: f64) -> String {
    let v = if v.abs() < 5e-7 { 0.0 } else { v };
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

fn write_rec(n: &Sexp, out: &mut String, depth: usize) {
    match n {
        Sexp::Atom(s, q) => {
            if *q {
                out.push('"');
                out.push_str(s);
                out.push('"');
            } else {
                out.push_str(s);
            }
        }
        Sexp::List(v) => {
            out.push('(');
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    // put nested lists of "big" nodes on new lines for readability
                    if matches!(c, Sexp::List(_)) && depth < 2 {
                        out.push('\n');
                        for _ in 0..=depth {
                            out.push('\t');
                        }
                    } else {
                        out.push(' ');
                    }
                }
                write_rec(c, out, depth + 1);
            }
            out.push(')');
        }
    }
}

pub fn parse(src: &str) -> Result<Sexp, String> {
    let mut stack: Vec<Vec<Sexp>> = vec![Vec::new()];
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'(' => {
                stack.push(Vec::new());
                i += 1;
            }
            b')' => {
                let list = stack.pop().ok_or("unbalanced ')'")?;
                stack
                    .last_mut()
                    .ok_or("unbalanced ')'")?
                    .push(Sexp::List(list));
                i += 1;
            }
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let s = src.get(start..i.min(bytes.len())).ok_or("bad string")?.to_string();
                i += 1;
                stack.last_mut().ok_or("parse error")?.push(Sexp::Atom(s, true));
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'('
                    && bytes[i] != b')'
                {
                    i += 1;
                }
                stack
                    .last_mut()
                    .ok_or("parse error")?
                    .push(Sexp::Atom(src[start..i].to_string(), false));
            }
        }
    }
    let mut top = stack.pop().ok_or("parse error")?;
    if !stack.is_empty() {
        return Err("unbalanced '('".into());
    }
    top.pop().ok_or_else(|| "empty document".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let src = r#"(kicad_pcb (version 20241229) (gr_text "a \"q\" \\n" (at 1 2 90) (layer "F.SilkS")))"#;
        let t = parse(src).unwrap();
        let mut o = String::new();
        t.write(&mut o);
        let t2 = parse(&o).unwrap();
        let mut o2 = String::new();
        t2.write(&mut o2);
        assert_eq!(o, o2);
        assert!(o.contains(r#""a \"q\" \\n""#));
    }
    #[test]
    fn nums() {
        assert_eq!(fmt_num(1.5), "1.5");
        assert_eq!(fmt_num(2.0), "2");
        assert_eq!(fmt_num(-0.0000001), "0");
    }
}
