pub fn looks_like_math(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    let has_op = t.chars().any(|c| "+-*/%^".contains(c));
    let only_allowed = t
        .chars()
        .all(|c| c.is_ascii_digit() || "+-*/%^().".contains(c) || c.is_whitespace());
    has_digit && has_op && only_allowed
}

pub fn eval(s: &str) -> Option<f64> {
    let mut p = Parser {
        chars: s.chars().collect(),
        pos: 0,
    };
    p.skip_ws();
    let r = p.parse_expr().ok()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return None;
    }
    if !r.is_finite() {
        return None;
    }
    Some(r)
}

pub fn format_result(r: f64) -> String {
    if r.fract() == 0.0 && r.abs() < 1e15 {
        format!("{}", r as i64)
    } else {
        format!("{}", r)
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_expr(&mut self) -> Result<f64, ()> {
        let mut v = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    v += self.parse_term()?;
                }
                Some('-') => {
                    self.pos += 1;
                    v -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Ok(v)
    }

    fn parse_term(&mut self) -> Result<f64, ()> {
        let mut v = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    v *= self.parse_factor()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let r = self.parse_factor()?;
                    if r == 0.0 {
                        return Err(());
                    }
                    v /= r;
                }
                Some('%') => {
                    self.pos += 1;
                    let r = self.parse_factor()?;
                    if r == 0.0 {
                        return Err(());
                    }
                    v %= r;
                }
                _ => break,
            }
        }
        Ok(v)
    }

    fn parse_factor(&mut self) -> Result<f64, ()> {
        self.skip_ws();
        let base = match self.peek() {
            Some('+') => {
                self.pos += 1;
                return self.parse_factor();
            }
            Some('-') => {
                self.pos += 1;
                return Ok(-self.parse_factor()?);
            }
            Some('(') => {
                self.pos += 1;
                let v = self.parse_expr()?;
                self.skip_ws();
                if self.peek() == Some(')') {
                    self.pos += 1;
                    v
                } else {
                    return Err(());
                }
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.parse_number()?,
            _ => return Err(()),
        };
        self.skip_ws();
        if self.peek() == Some('^') {
            self.pos += 1;
            let exp = self.parse_factor()?;
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_number(&mut self) -> Result<f64, ()> {
        let start = self.pos;
        let mut seen_dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == '.' && !seen_dot {
                seen_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>().map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_math() {
        assert_eq!(eval("2+2").unwrap(), 4.0);
        assert_eq!(eval("10-3").unwrap(), 7.0);
        assert_eq!(eval("4*5").unwrap(), 20.0);
        assert_eq!(eval("20/8").unwrap(), 2.5);
        assert_eq!(eval("9%4").unwrap(), 1.0);
        assert_eq!(eval("2^10").unwrap(), 1024.0);
    }

    #[test]
    fn precedence_and_parens() {
        assert_eq!(eval("2+3*4").unwrap(), 14.0);
        assert_eq!(eval("(2+3)*4").unwrap(), 20.0);
        assert_eq!(eval("-(3+2)").unwrap(), -5.0);
        assert_eq!(eval("2*(1+2)^2").unwrap(), 18.0);
    }

    #[test]
    fn floats() {
        assert_eq!(eval("3.14*2").unwrap(), 6.28);
    }

    #[test]
    fn rejects_garbage() {
        assert!(eval("2++").is_none());
        assert!(eval("firefox").is_none());
        assert!(eval("2+2 ").is_none() == false); // trailing ws handled
        assert!(eval("").is_none());
    }

    #[test]
    fn detection() {
        assert!(looks_like_math("2+2"));
        assert!(!looks_like_math("firefox"));
        assert!(!looks_like_math("42"));
        assert!(looks_like_math("(1+2)*3"));
    }

    #[test]
    fn formatting() {
        assert_eq!(format_result(4.0), "4");
        assert_eq!(format_result(2.5), "2.5");
        assert_eq!(format_result(-3.0), "-3");
    }
}

