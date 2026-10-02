//! Molang: the small expression language Bedrock animations use for values
//! that change over time, e.g. `math.sin(query.anim_time * 360) * 30`.
//! See docs/animation.md#molang for what is supported.

use std::collections::HashMap;

use crate::gomath;

/// A compiled Molang expression or script.
#[derive(Clone, Debug)]
pub(crate) struct Molang {
    stmts: Vec<Stmt>,
}

/// What an expression can read: queries by name (lower case, without the
/// `query.` prefix) and the script's variables, which it can also set.
#[derive(Default)]
pub(crate) struct Env {
    pub queries: HashMap<&'static str, f64>,
    pub variables: HashMap<String, f64>,
}

impl Env {
    fn get(&self, name: &str) -> f64 {
        if let Some(q) = name.strip_prefix("query.") {
            return self.queries.get(q).copied().unwrap_or(0.0);
        }
        if name.starts_with("variable.")
            || name.starts_with("temp.")
            || name.starts_with("context.")
        {
            return self.variables.get(name).copied().unwrap_or(0.0);
        }
        0.0
    }
}

#[derive(Clone, Debug)]
enum Stmt {
    Expr(Expr),
    Assign(String, Expr),
    Return(Expr),
}

#[derive(Clone, Debug)]
enum Expr {
    Number(f64),
    Name(String),
    Unary(char, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

impl Molang {
    pub fn constant(v: f64) -> Molang {
        Molang {
            stmts: vec![Stmt::Expr(Expr::Number(v))],
        }
    }

    /// Parses `src`. A number or empty string is a constant.
    pub fn compile(src: &str) -> Result<Molang, String> {
        let mut p = Parser {
            src,
            toks: lex(src)?,
            i: 0,
        };
        let mut stmts = Vec::new();
        while !p.at(Kind::Eof, "") {
            if p.at(Kind::Punct, ";") {
                p.i += 1;
                continue;
            }
            stmts.push(p.statement()?);
            if !p.at(Kind::Eof, "") && !p.at(Kind::Punct, ";") {
                return Err(p.error("expected ; or the end"));
            }
        }
        Ok(Molang { stmts })
    }

    /// Runs it. A single expression is its own value; a script of statements
    /// is the value of its return statement, or 0.
    pub fn eval(&self, env: &mut Env) -> f64 {
        if let [Stmt::Expr(e)] = self.stmts.as_slice() {
            return finite(e.eval(env));
        }
        for st in &self.stmts {
            match st {
                Stmt::Assign(name, e) => {
                    let v = finite(e.eval(env));
                    env.variables.insert(name.clone(), v);
                }
                Stmt::Return(e) => return finite(e.eval(env)),
                Stmt::Expr(e) => {
                    e.eval(env);
                }
            }
        }
        0.0
    }
}

/// Keeps a NaN or infinity (a division by zero, say) from reaching the
/// renderer, where it would make a vertex vanish.
fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

// ---- lexing ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Eof,
    Num,
    Ident,
    Punct,
}

#[derive(Clone, Debug)]
struct Tok {
    kind: Kind,
    text: String,
    num: f64,
    pos: usize,
}

// Go reads the source a byte at a time and asks unicode about each byte as a
// rune, so a byte past ASCII is judged as the Latin-1 character it would be.
fn is_letter(b: u8) -> bool {
    (b as char).is_alphabetic()
}

fn is_space(b: u8) -> bool {
    (b as char).is_whitespace()
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let bytes = s.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    let quoted = |s: &str| format!("{s:?}");
    while i < bytes.len() {
        let c = bytes[i];
        if is_space(c) {
            i += 1;
        } else if c.is_ascii_digit()
            || c == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()
        {
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'f' || bytes[j] == b'F') {
                j += 1; // 1.5f, as some files write
            }
            let text = &s[i..j];
            let v: f64 = text
                .trim_end_matches(['f', 'F'])
                .parse()
                .map_err(|_| format!("molang {} at {i}: bad number {}", quoted(s), quoted(text)))?;
            toks.push(Tok {
                kind: Kind::Num,
                text: String::new(),
                num: v,
                pos: i,
            });
            i = j;
        } else if is_letter(c) || c == b'_' {
            let mut j = i;
            while j < bytes.len()
                && (is_letter(bytes[j])
                    || bytes[j].is_ascii_digit()
                    || bytes[j] == b'_'
                    || bytes[j] == b'.')
            {
                j += 1;
            }
            toks.push(Tok {
                kind: Kind::Ident,
                text: normalize_name(&s[i..j]),
                num: 0.0,
                pos: i,
            });
            i = j;
        } else {
            if i + 1 < bytes.len() {
                let two = &bytes[i..i + 2];
                if [b"==", b"!=", b"<=", b">=", b"&&", b"||", b"??", b"->"]
                    .iter()
                    .any(|t| t[..] == *two)
                {
                    toks.push(Tok {
                        kind: Kind::Punct,
                        text: String::from_utf8_lossy(two).into(),
                        num: 0.0,
                        pos: i,
                    });
                    i += 2;
                    continue;
                }
            }
            if b"+-*/()<>!?:,;=".contains(&c) {
                toks.push(Tok {
                    kind: Kind::Punct,
                    text: (c as char).to_string(),
                    num: 0.0,
                    pos: i,
                });
                i += 1;
                continue;
            }
            return Err(format!(
                "molang {} at {i}: unexpected {}",
                quoted(s),
                quoted(&(c as char).to_string())
            ));
        }
    }
    toks.push(Tok {
        kind: Kind::Eof,
        text: String::new(),
        num: 0.0,
        pos: bytes.len(),
    });
    Ok(toks)
}

/// Lower-cases a name and expands Molang's short prefixes (q., v., t., c.),
/// so `Math.Cos` and `math.cos`, `q.anim_time` and `query.anim_time` are one
/// name each.
fn normalize_name(s: &str) -> String {
    let s = s.to_lowercase();
    for (short, long) in [
        ("q.", "query."),
        ("v.", "variable."),
        ("t.", "temp."),
        ("c.", "context."),
    ] {
        if let Some(rest) = s.strip_prefix(short) {
            return format!("{long}{rest}");
        }
    }
    s
}

// ---- parsing ----

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Tok>,
    i: usize,
}

const LEVELS: [&[&str]; 6] = [
    &["||"],
    &["&&"],
    &["==", "!="],
    &["<", ">", "<=", ">="],
    &["+", "-"],
    &["*", "/"],
];

impl Parser<'_> {
    fn error(&self, msg: &str) -> String {
        let pos = self.toks.get(self.i).map_or(self.src.len(), |t| t.pos);
        format!("molang {:?} at {pos}: {msg}", self.src)
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.i]
    }

    fn at(&self, kind: Kind, text: &str) -> bool {
        let t = self.peek();
        t.kind == kind && (text.is_empty() || t.text == text)
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        if self.at(Kind::Ident, "return") {
            self.i += 1;
            return Ok(Stmt::Return(self.expr()?));
        }
        if self.at(Kind::Ident, "") {
            let next = &self.toks[self.i + 1];
            if next.kind == Kind::Punct && next.text == "=" {
                let name = self.toks[self.i].text.clone();
                self.i += 2;
                return Ok(Stmt::Assign(name, self.expr()?));
            }
        }
        Ok(Stmt::Expr(self.expr()?))
    }

    // Precedence, loosest first: ?? then ?: then || && then comparisons,
    // + -, * /, unary.
    fn expr(&mut self) -> Result<Expr, String> {
        let mut l = self.ternary()?;
        while self.at(Kind::Punct, "??") {
            self.i += 1;
            let r = self.ternary()?;
            l = Expr::Binary("??", Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn ternary(&mut self) -> Result<Expr, String> {
        let cond = self.binary(0)?;
        if !self.at(Kind::Punct, "?") {
            return Ok(cond);
        }
        self.i += 1;
        let yes = self.ternary()?;
        // "a ? b" with no ":" is 0 when false.
        let mut no = Expr::Number(0.0);
        if self.at(Kind::Punct, ":") {
            self.i += 1;
            no = self.ternary()?;
        }
        Ok(Expr::Ternary(Box::new(cond), Box::new(yes), Box::new(no)))
    }

    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.binary(level + 1)?;
        loop {
            let t = self.peek();
            let op = match LEVELS[level]
                .iter()
                .find(|op| t.kind == Kind::Punct && t.text == **op)
            {
                Some(op) => *op,
                None => return Ok(l),
            };
            self.i += 1;
            let r = self.binary(level + 1)?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        for op in ['-', '!', '+'] {
            if self.at(Kind::Punct, &op.to_string()) {
                self.i += 1;
                return Ok(Expr::Unary(op, Box::new(self.unary()?)));
            }
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let t = self.toks[self.i].clone();
        self.i += 1;
        match t.kind {
            Kind::Num => return Ok(Expr::Number(t.num)),
            Kind::Punct if t.text == "(" => {
                let e = self.expr()?;
                if !self.at(Kind::Punct, ")") {
                    return Err(self.error("expected )"));
                }
                self.i += 1;
                return Ok(e);
            }
            Kind::Ident => {
                match t.text.as_str() {
                    "true" => return Ok(Expr::Number(1.0)),
                    "false" => return Ok(Expr::Number(0.0)),
                    "math.pi" => return Ok(Expr::Number(std::f64::consts::PI)),
                    _ => {}
                }
                if self.at(Kind::Punct, "(") {
                    self.i += 1;
                    let mut args = Vec::new();
                    while !self.at(Kind::Punct, ")") {
                        if self.at(Kind::Eof, "") {
                            return Err(self.error("unexpected \"\""));
                        }
                        args.push(self.expr()?);
                        if self.at(Kind::Punct, ",") {
                            self.i += 1;
                        } else if !self.at(Kind::Punct, ")") {
                            return Err(self.error("expected , or )"));
                        }
                    }
                    self.i += 1;
                    if t.text.starts_with("math.") && math_func(&t.text).is_none() {
                        return Err(self.error(&format!("unknown function {}", t.text)));
                    }
                    return Ok(Expr::Call(t.text, args));
                }
                return Ok(Expr::Name(t.text));
            }
            _ => {}
        }
        self.i -= 1;
        Err(self.error(&format!("unexpected {:?}", t.text)))
    }
}

// ---- evaluation ----

fn truth(v: f64) -> bool {
    v != 0.0
}

fn b2f(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

impl Expr {
    fn eval(&self, env: &mut Env) -> f64 {
        match self {
            Expr::Number(n) => *n,
            Expr::Name(n) => env.get(n),
            Expr::Unary(op, e) => {
                let v = e.eval(env);
                match op {
                    '-' => -v,
                    '!' => b2f(!truth(v)),
                    _ => v,
                }
            }
            Expr::Binary(op, l, r) => match *op {
                // Short-circuiting.
                "&&" => b2f(truth(l.eval(env)) && truth(r.eval(env))),
                "||" => b2f(truth(l.eval(env)) || truth(r.eval(env))),
                // Every name here has a value (unknown ones are 0), so the
                // left side always stands.
                "??" => l.eval(env),
                _ => {
                    let (l, r) = (l.eval(env), r.eval(env));
                    match *op {
                        "+" => l + r,
                        "-" => l - r,
                        "*" => l * r,
                        "/" => {
                            if r == 0.0 {
                                0.0
                            } else {
                                l / r
                            }
                        }
                        "==" => b2f(l == r),
                        "!=" => b2f(l != r),
                        "<" => b2f(l < r),
                        ">" => b2f(l > r),
                        "<=" => b2f(l <= r),
                        ">=" => b2f(l >= r),
                        _ => 0.0,
                    }
                }
            },
            Expr::Ternary(c, y, n) => {
                if truth(c.eval(env)) {
                    y.eval(env)
                } else {
                    n.eval(env)
                }
            }
            Expr::Call(name, args) => {
                let vals: Vec<f64> = args.iter().map(|a| a.eval(env)).collect();
                match math_func(name) {
                    Some(f) => f(&vals),
                    None => 0.0, // a query function this library does not model
                }
            }
        }
    }
}

const DEG: f64 = std::f64::consts::PI / 180.0;

fn arg(a: &[f64], i: usize) -> f64 {
    a.get(i).copied().unwrap_or(0.0)
}

/// Go's math.Mod: the remainder with the sign of x, as fmod.
fn fmod(x: f64, y: f64) -> f64 {
    x % y
}

type MathFn = fn(&[f64]) -> f64;

/// Molang's math functions. Trigonometry is in degrees, as in Bedrock.
/// random is the middle of its range, so a render is repeatable.
fn math_func(name: &str) -> Option<MathFn> {
    Some(match name {
        "math.sin" => |a| gomath::sin(arg(a, 0) * DEG),
        "math.cos" => |a| gomath::cos(arg(a, 0) * DEG),
        "math.asin" => |a| gomath::asin(arg(a, 0)) / DEG,
        "math.acos" => |a| gomath::acos(arg(a, 0)) / DEG,
        "math.atan" => |a| gomath::atan(arg(a, 0)) / DEG,
        "math.atan2" => |a| gomath::atan2(arg(a, 0), arg(a, 1)) / DEG,
        "math.abs" => |a| arg(a, 0).abs(),
        "math.ceil" => |a| arg(a, 0).ceil(),
        "math.floor" => |a| arg(a, 0).floor(),
        "math.round" => |a| arg(a, 0).round(),
        "math.trunc" => |a| arg(a, 0).trunc(),
        "math.sqrt" => |a| arg(a, 0).sqrt(),
        "math.exp" => |a| arg(a, 0).exp(),
        "math.ln" => |a| arg(a, 0).ln(),
        "math.pow" => |a| arg(a, 0).powf(arg(a, 1)),
        "math.mod" => |a| {
            if arg(a, 1) == 0.0 {
                0.0
            } else {
                fmod(arg(a, 0), arg(a, 1))
            }
        },
        "math.min" => |a| gomath::min(arg(a, 0), arg(a, 1)),
        "math.max" => |a| gomath::max(arg(a, 0), arg(a, 1)),
        "math.clamp" => |a| gomath::max(arg(a, 1), gomath::min(arg(a, 2), arg(a, 0))),
        "math.lerp" => |a| arg(a, 0) + (arg(a, 1) - arg(a, 0)) * arg(a, 2),
        "math.lerprotate" => |a| {
            let (from, to) = (arg(a, 0), arg(a, 1));
            let d = fmod(to - from + 540.0, 360.0) - 180.0;
            from + d * arg(a, 2)
        },
        "math.hermite_blend" => |a| {
            let t = arg(a, 0);
            3.0 * t * t - 2.0 * t * t * t
        },
        "math.random" => |a| (arg(a, 0) + arg(a, 1)) / 2.0,
        "math.random_integer" => |a| ((arg(a, 0) + arg(a, 1)) / 2.0).round(),
        "math.die_roll" => |a| arg(a, 0) * (arg(a, 1) + arg(a, 2)) / 2.0,
        "math.die_roll_integer" => |a| (arg(a, 0) * (arg(a, 1) + arg(a, 2)) / 2.0).round(),
        "math.min_angle" => |a| {
            let mut v = fmod(arg(a, 0), 360.0);
            if v >= 180.0 {
                v -= 360.0;
            } else if v < -180.0 {
                v += 360.0;
            }
            v
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> f64 {
        let mut env = Env::default();
        env.queries.insert("anim_time", 0.5);
        Molang::compile(src).unwrap().eval(&mut env)
    }

    #[test]
    fn expressions() {
        assert_eq!(eval("1 + 2 * 3"), 7.0);
        assert_eq!(eval("(1 + 2) * 3"), 9.0);
        assert_eq!(eval("q.anim_time * 2"), 1.0);
        assert_eq!(eval("Math.Sin(90)"), 1.0);
        assert_eq!(eval("1 > 2 ? 5 : 6"), 6.0);
        assert_eq!(eval("0 ? 5"), 0.0);
        assert_eq!(eval("v.x = 3; v.y = v.x * 2; return v.y + 1;"), 7.0);
        assert_eq!(eval("1 / 0"), 0.0);
        assert_eq!(eval("-2 - -3"), 1.0);
        assert_eq!(eval("1.5f * 2"), 3.0);
        assert_eq!(eval("query.unknown ?? 4"), 0.0);
        assert_eq!(eval("math.clamp(5, 0, 2)"), 2.0);
        assert_eq!(eval("!0 && 1"), 1.0);
    }

    #[test]
    fn errors() {
        for src in ["1 +", "math.nope(1)", "(1", "1 $ 2", "f(1"] {
            assert!(Molang::compile(src).is_err(), "{src}");
        }
    }
}
