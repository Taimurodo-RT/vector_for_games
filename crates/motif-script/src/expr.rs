//! Tiny arithmetic evaluator for `{...}` expressions: numbers, variables, + - * / %, parentheses.

pub fn eval(src: &str, var: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    let mut p = P { s: src.as_bytes(), i: 0, src, var };
    let v = p.sum()?;
    p.ws();
    if p.i < p.s.len() {
        return Err(format!("непонятное выражение «{src}»"));
    }
    Ok(v)
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    src: &'a str,
    var: &'a dyn Fn(&str) -> Option<f64>,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            if self.eat(b'+') {
                v += self.product()?;
            } else if self.eat(b'-') {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else if self.eat(b'%') {
                v = v.rem_euclid(self.unary()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        if self.eat(b'-') {
            return Ok(-self.unary()?);
        }
        if self.eat(b'(') {
            let v = self.sum()?;
            if !self.eat(b')') {
                return Err(format!("не закрыта скобка в «{}»", self.src));
            }
            return Ok(v);
        }
        self.ws();
        let start = self.i;
        let c = *self.s.get(self.i).ok_or_else(|| format!("выражение «{}» оборвано", self.src))?;
        if c.is_ascii_digit() || c == b'.' {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                self.i += 1;
            }
            return self.src[start..self.i].parse().map_err(|_| format!("неверное число в «{}»", self.src));
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                self.i += 1;
            }
            let name = &self.src[start..self.i];
            return (self.var)(name).ok_or_else(|| format!("неизвестное числовое имя «{name}»"));
        }
        Err(format!("непонятное выражение «{}»", self.src))
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn precedence_and_vars() {
        let var = |n: &str| (n == "i").then_some(3.0);
        assert_eq!(eval("1 + 2 * 3", &var), Ok(7.0));
        assert_eq!(eval("(1 + 2) * -i", &var), Ok(-9.0));
        assert_eq!(eval("i * 24 % 10", &var), Ok(2.0));
        assert!(eval("j + 1", &var).is_err());
    }
}
