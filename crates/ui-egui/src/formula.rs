//! RevuCraft: calculated columns in the Markups list, as BIMCraft's schedules (and Revit's)
//! calculate theirs: a formula over the row's other values, such as `Value * 1.1` for cable with
//! slack or `Count * 85` for a cost. Names are the list's columns (`Value`, `Count`, `Page`, and
//! yours); one with spaces goes in brackets, `[Drop count] * 2`. `+ - * /`, brackets and numbers;
//! anything else is an error shown in the cell, and a value that isn't a number counts as none.

/// The longest formula read, and how deep brackets nest.
const MAX_LEN: usize = 400;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Op(char),
    Open,
    Close,
}

fn tokens(src: &str) -> Result<Vec<Tok>, String> {
    if src.chars().count() > MAX_LEN {
        return Err("the formula is too long".into());
    }
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_ascii_digit() || c == '.' {
            let mut s = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() || d == '.' {
                    s.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Num(s.parse().map_err(|_| format!("{s} isn't a number"))?));
        } else if c == '[' {
            chars.next();
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some(']') => break,
                    Some(d) => s.push(d),
                    None => return Err("a [ has no ]".into()),
                }
            }
            out.push(Tok::Name(s.trim().to_string()));
        } else if c.is_alphabetic() || c == '_' {
            let mut s = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_alphanumeric() || d == '_' {
                    s.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Name(s));
        } else if "+-*/".contains(c) {
            chars.next();
            out.push(Tok::Op(c));
        } else if c == '(' {
            chars.next();
            out.push(Tok::Open);
        } else if c == ')' {
            chars.next();
            out.push(Tok::Close);
        } else {
            return Err(format!("{c} isn't understood"));
        }
    }
    Ok(out)
}

struct Parser<'a, F: Fn(&str) -> Option<f64>> {
    toks: &'a [Tok],
    at: usize,
    value_of: &'a F,
}

impl<F: Fn(&str) -> Option<f64>> Parser<'_, F> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn sum(&mut self, depth: usize) -> Result<f64, String> {
        let mut v = self.product(depth)?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let r = self.product(depth)?;
            v = if op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn product(&mut self, depth: usize) -> Result<f64, String> {
        let mut v = self.unary(depth)?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let r = self.unary(depth)?;
            v = if op == '*' {
                v * r
            } else if r == 0.0 {
                return Err("division by zero".into());
            } else {
                v / r
            };
        }
        Ok(v)
    }

    fn unary(&mut self, depth: usize) -> Result<f64, String> {
        if depth > MAX_DEPTH {
            return Err("brackets nest too deep".into());
        }
        match self.peek().cloned() {
            Some(Tok::Op('-')) => {
                self.at += 1;
                Ok(-self.unary(depth + 1)?)
            }
            Some(Tok::Op('+')) => {
                self.at += 1;
                self.unary(depth + 1)
            }
            Some(Tok::Num(n)) => {
                self.at += 1;
                Ok(n)
            }
            Some(Tok::Name(name)) => {
                self.at += 1;
                (self.value_of)(&name).ok_or_else(|| format!("{name} has no number"))
            }
            Some(Tok::Open) => {
                self.at += 1;
                let v = self.sum(depth + 1)?;
                match self.peek() {
                    Some(Tok::Close) => {
                        self.at += 1;
                        Ok(v)
                    }
                    _ => Err("a ( has no )".into()),
                }
            }
            _ => Err("a value is missing".into()),
        }
    }
}

/// The value of `formula`, its names looked up with `value_of`.
pub fn evaluate(formula: &str, value_of: impl Fn(&str) -> Option<f64>) -> Result<f64, String> {
    let toks = tokens(formula)?;
    if toks.is_empty() {
        return Err("the formula is empty".into());
    }
    let mut p = Parser { toks: &toks, at: 0, value_of: &value_of };
    let v = p.sum(0)?;
    if p.at != toks.len() {
        return Err("something follows the formula".into());
    }
    if v.is_finite() { Ok(v) } else { Err("the result is out of range".into()) }
}

/// The number a value starts with (`12`, `12.5 ft`, `-3`), if it does.
pub fn leading_number(s: &str) -> Option<f64> {
    let s = s.trim();
    let end = s.char_indices().find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+')))).map_or(s.len(), |(i, _)| i);
    s.get(..end).and_then(|n| n.parse::<f64>().ok()).filter(|v| v.is_finite())
}

/// A calculated value as the list shows it: up to two decimals, no trailing zeros.
pub fn show(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(name: &str) -> Option<f64> {
        match name.to_ascii_lowercase().as_str() {
            "value" => Some(120.0),
            "count" => Some(1.0),
            "drop count" => Some(4.0),
            _ => None,
        }
    }

    #[test]
    fn formulas_calculate_over_the_rows_values() {
        assert_eq!(evaluate("Value * 1.1", vars).map(show), Ok("132".into()));
        assert_eq!(evaluate("[Drop count] * 2 + 1", vars), Ok(9.0));
        assert_eq!(evaluate("(Value + 30) / 3", vars), Ok(50.0));
        assert_eq!(evaluate("-Count + 2 * -3", vars), Ok(-7.0));
        assert_eq!(evaluate("2 * (3 + 4) - 1", vars), Ok(13.0));
    }

    #[test]
    fn bad_formulas_are_errors_not_crashes() {
        for bad in ["", "Value *", "(1 + 2", "1 + 2)", "Nope + 1", "1 / 0", "[open", "3 $ 4", "1..2", "Value Value"] {
            assert!(evaluate(bad, vars).is_err(), "{bad}");
        }
        assert!(evaluate(&"(".repeat(100), vars).is_err(), "deep brackets");
        assert!(evaluate(&"1+".repeat(300), vars).is_err(), "too long");
        assert!(evaluate(&format!("{} * {}", "9".repeat(200), "9".repeat(150)), vars).is_err(), "out of range");
    }

    #[test]
    fn values_lend_their_leading_numbers() {
        assert_eq!(leading_number("12"), Some(12.0));
        assert_eq!(leading_number(" 12.5 ft"), Some(12.5));
        assert_eq!(leading_number("-3"), Some(-3.0));
        assert_eq!(leading_number("Cat6"), None);
        assert_eq!(leading_number(""), None);
        assert_eq!(show(2.0), "2");
        assert_eq!(show(2.346), "2.35");
        assert_eq!(show(-0.001), "0");
    }
}
