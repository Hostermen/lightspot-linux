// calculator.rs — A tiny recursive-descent math evaluator.
//
// Used by the search module: when the user's query "looks like math", we
// evaluate it and offer the result as a copy-to-clipboard item. Supports
// + - * / % ^, parentheses, unary +/-, and decimal numbers. No functions,
// no variables — deliberately minimal so it never over-reaches into file
// or app queries that happen to contain digits/operators.

/// Heuristic: does this string look like a math expression we should try
/// to evaluate? Requires at least one digit, at least one operator, and
/// only allowed characters (digits, operators, parentheses, dot, space).
pub fn looks_like_math(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false; // nothing to evaluate
    }
    // Must contain at least one digit.
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    // Must contain at least one operator we support.
    let has_op = t.chars().any(|c| "+-*/%^".contains(c));
    // Must contain ONLY allowed characters (reject "firefox", "2 + a", etc.).
    let only_allowed = t
        .chars()
        .all(|c| c.is_ascii_digit() || "+-*/%^().".contains(c) || c.is_whitespace());
    has_digit && has_op && only_allowed
}

/// Evaluate `s` as a math expression. Returns `None` on any parse/eval
/// error or if the result is not finite (NaN/Infinity).
pub fn eval(s: &str) -> Option<f64> {
    let mut p = Parser {
        chars: s.chars().collect(), // materialize for easy indexing
        pos: 0,                     // current parse cursor
    };
    p.skip_ws();
    let r = p.parse_expr().ok()?; // top-level expression
    p.skip_ws();
    // Reject trailing garbage after a valid expression.
    if p.pos != p.chars.len() {
        return None;
    }
    // Reject non-finite results (division produced NaN/Inf, etc.).
    if !r.is_finite() {
        return None;
    }
    Some(r)
}

/// Format a result for display: integers without a decimal point, floats as-is.
pub fn format_result(r: f64) -> String {
    if r.fract() == 0.0 && r.abs() < 1e15 {
        // Whole number within safe integer range → render as integer.
        format!("{}", r as i64)
    } else {
        // Otherwise render the float directly.
        format!("{}", r)
    }
}

// The parser state: the input as chars plus a cursor position.
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    /// Peek at the current character without consuming it.
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Skip any whitespace at the current position.
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    // expr := term (('+' | '-') term)*
    // Lowest precedence: left-associative addition / subtraction.
    fn parse_expr(&mut self) -> Result<f64, ()> {
        let mut v = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1; // consume operator
                    v += self.parse_term()?;
                }
                Some('-') => {
                    self.pos += 1;
                    v -= self.parse_term()?;
                }
                _ => break, // no more +/-
            }
        }
        Ok(v)
    }

    // term := factor (('*' | '/' | '%') factor)*
    // Higher precedence than expr: multiplication, division, modulo.
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
                        return Err(()); // division by zero
                    }
                    v /= r;
                }
                Some('%') => {
                    self.pos += 1;
                    let r = self.parse_factor()?;
                    if r == 0.0 {
                        return Err(()); // modulo by zero
                    }
                    v %= r;
                }
                _ => break, // no more */%
            }
        }
        Ok(v)
    }

    // factor := ('+' | '-') factor | '(' expr ')' | number ('^' factor)?
    // Highest precedence: unary signs, parentheses, exponentiation (right-assoc).
    fn parse_factor(&mut self) -> Result<f64, ()> {
        self.skip_ws();
        let base = match self.peek() {
            // Unary plus — just return the factor after it.
            Some('+') => {
                self.pos += 1;
                return self.parse_factor();
            }
            // Unary minus — negate the factor after it.
            Some('-') => {
                self.pos += 1;
                return Ok(-self.parse_factor()?);
            }
            // Parenthesized sub-expression: recurse back to the top level.
            Some('(') => {
                self.pos += 1; // consume '('
                let v = self.parse_expr()?;
                self.skip_ws();
                if self.peek() == Some(')') {
                    self.pos += 1; // consume ')'
                    v
                } else {
                    return Err(()); // missing closing paren
                }
            }
            // A number literal starts with a digit or a dot.
            Some(c) if c.is_ascii_digit() || c == '.' => self.parse_number()?,
            _ => return Err(()), // unexpected token
        };
        self.skip_ws();
        // Optional exponent: a^b — recurses into parse_factor so a^b^c is right-assoc.
        if self.peek() == Some('^') {
            self.pos += 1;
            let exp = self.parse_factor()?;
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    // number := [0-9]+ ('.' [0-9]*)? | '.' [0-9]+
    // Consumes the longest run of digits and at most one decimal point.
    fn parse_number(&mut self) -> Result<f64, ()> {
        let start = self.pos;
        let mut seen_dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == '.' && !seen_dot {
                seen_dot = true; // allow exactly one '.'
                self.pos += 1;
            } else {
                break; // end of number
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>().map_err(|_| ()) // e.g. "." alone fails to parse
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
        assert_eq!(eval("2+3*4").unwrap(), 14.0); // * before +
        assert_eq!(eval("(2+3)*4").unwrap(), 20.0); // parens override
        assert_eq!(eval("-(3+2)").unwrap(), -5.0); // unary minus
        assert_eq!(eval("2*(1+2)^2").unwrap(), 18.0); // ^ before *
    }

    #[test]
    fn floats() {
        assert_eq!(eval("3.14*2").unwrap(), 6.28);
    }

    #[test]
    fn rejects_garbage() {
        assert!(eval("2++").is_none()); // dangling operator
        assert!(eval("firefox").is_none()); // not a number
        assert!(eval("2+2 ").is_none() == false); // trailing ws handled
        assert!(eval("").is_none()); // empty input
    }

    #[test]
    fn detection() {
        assert!(looks_like_math("2+2")); // math
        assert!(!looks_like_math("firefox")); // no digit/op
        assert!(!looks_like_math("42")); // digit but no op
        assert!(looks_like_math("(1+2)*3")); // full expression
    }

    #[test]
    fn formatting() {
        assert_eq!(format_result(4.0), "4"); // integer → no dot
        assert_eq!(format_result(2.5), "2.5"); // float → as-is
        assert_eq!(format_result(-3.0), "-3"); // negative integer
    }
}
