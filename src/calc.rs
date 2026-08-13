/// Tiny recursive-descent expression evaluator: + - * / % ^ ( ) and unary minus.
/// Returns None unless the whole input is a valid expression containing at
/// least one operator (so plain numbers or app names never trigger it).

pub fn eval(input: &str) -> Option<f64> {
    let mut s: Vec<char> = input.chars().filter(|c| !c.is_whitespace()).collect();
    if s.last() == Some(&'=') {
        s.pop(); // tolerate the "5*5=" typing habit
    }
    if s.is_empty() || !s.iter().all(|c| "0123456789.+-*/%^()".contains(*c)) {
        return None;
    }
    if !s.iter().any(|c| "+-*/%^".contains(*c)) {
        return None;
    }
    let mut p = Parser { s: &s, i: 0 };
    let v = p.expr()?;
    if p.i != p.s.len() || !v.is_finite() {
        return None;
    }
    Some(v)
}

pub fn format(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.10}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        while let Some(op @ ('*' | '/' | '%')) = self.peek() {
            self.i += 1;
            let r = self.unary()?;
            v = match op {
                '*' => v * r,
                '/' => v / r,
                _ => v % r,
            };
        }
        Some(v)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.peek() == Some('-') {
            self.i += 1;
            return Some(-self.unary()?);
        }
        self.power()
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.peek() == Some('^') {
            self.i += 1;
            let exp = self.unary()?; // right-associative
            return Some(base.powf(exp));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            '(' => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(')') {
                    return None;
                }
                self.i += 1;
                Some(v)
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.') {
                    self.i += 1;
                }
                let text: String = self.s[start..self.i].iter().collect();
                text.parse().ok()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{eval, format};

    #[test]
    fn precedence() {
        assert_eq!(eval("2+2*4"), Some(10.0));
        assert_eq!(eval("(2+2)*4"), Some(16.0));
    }

    #[test]
    fn power_right_assoc() {
        assert_eq!(eval("2^3^2"), Some(512.0));
    }

    #[test]
    fn unary_and_percent() {
        assert_eq!(eval("-3+10"), Some(7.0));
        assert_eq!(eval("10%3"), Some(1.0));
    }

    #[test]
    fn trailing_equals() {
        assert_eq!(eval("5*5="), Some(25.0));
        assert_eq!(eval("5*5=="), None); // only one, at the end
    }

    #[test]
    fn rejects_non_math() {
        assert_eq!(eval("7zip"), None);
        assert_eq!(eval("42"), None); // plain number: not an expression
        assert_eq!(eval("2+"), None);
        assert_eq!(eval("1/0"), None); // infinity filtered
    }

    #[test]
    fn formatting() {
        assert_eq!(format(10.0), "10");
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(1.0 / 3.0), "0.3333333333");
    }
}
