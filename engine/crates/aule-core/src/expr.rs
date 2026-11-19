//! Evaluador mínimo de expresiones con fracciones, para verificar el contenido.
//! Soporta enteros, `+ - * /`, paréntesis y una comparación (`< > = <= >=`).
//! Ej.: `"3/4 + 1/6"`, `"(2/3) * 12"`, `"2/3 > 3/5"`.

use crate::fraccion::Fraccion;

#[derive(Debug, Clone, PartialEq)]
pub enum Valor {
    Num(Fraccion),
    Bool(bool),
}

pub fn evaluar(s: &str) -> Result<Valor, String> {
    for op in ["<=", ">=", "<", ">", "="] {
        if let Some((a, b)) = s.split_once(op) {
            let x = evaluar_num(a)?;
            let y = evaluar_num(b)?;
            let r = match op {
                "<=" => x <= y,
                ">=" => x >= y,
                "<" => x < y,
                ">" => x > y,
                _ => x == y,
            };
            return Ok(Valor::Bool(r));
        }
    }
    evaluar_num(s).map(Valor::Num)
}

pub fn evaluar_num(s: &str) -> Result<Fraccion, String> {
    let toks = tokenizar(s)?;
    let mut p = Parser { t: toks, i: 0 };
    let v = p.suma()?;
    if p.i != p.t.len() {
        return Err(format!("sobran símbolos en '{s}'"));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    N(i64),
    Op(char),
    Abre,
    Cierra,
}

fn tokenizar(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = vec![];
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let mut n = 0i64;
            while i < cs.len() && cs[i].is_ascii_digit() {
                n = n * 10 + cs[i].to_digit(10).unwrap() as i64;
                i += 1;
            }
            out.push(Tok::N(n));
        } else if "+-*/×÷".contains(c) {
            let c = match c {
                '×' => '*',
                '÷' => '/',
                o => o,
            };
            out.push(Tok::Op(c));
            i += 1;
        } else if c == '(' {
            out.push(Tok::Abre);
            i += 1;
        } else if c == ')' {
            out.push(Tok::Cierra);
            i += 1;
        } else {
            return Err(format!("símbolo inesperado '{c}' en '{s}'"));
        }
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn suma(&mut self) -> Result<Fraccion, String> {
        let mut v = self.prod()?;
        while let Some(Tok::Op(c)) = self.t.get(self.i).cloned() {
            if c != '+' && c != '-' {
                break;
            }
            self.i += 1;
            let w = self.prod()?;
            v = if c == '+' { v + w } else { v - w };
        }
        Ok(v)
    }
    fn prod(&mut self) -> Result<Fraccion, String> {
        let mut v = self.atomo()?;
        while let Some(Tok::Op(c)) = self.t.get(self.i).cloned() {
            if c != '*' && c != '/' {
                break;
            }
            self.i += 1;
            let w = self.atomo()?;
            v = if c == '*' {
                v * w
            } else {
                if w == Fraccion::entero(0) {
                    return Err("división entre cero".into());
                }
                v / w
            };
        }
        Ok(v)
    }
    fn atomo(&mut self) -> Result<Fraccion, String> {
        match self.t.get(self.i).cloned() {
            Some(Tok::N(n)) => {
                self.i += 1;
                Ok(Fraccion::entero(n))
            }
            Some(Tok::Op('-')) => {
                self.i += 1;
                Ok(-self.atomo()?)
            }
            Some(Tok::Abre) => {
                self.i += 1;
                let v = self.suma()?;
                if self.t.get(self.i) != Some(&Tok::Cierra) {
                    return Err("falta ')'".into());
                }
                self.i += 1;
                Ok(v)
            }
            otro => Err(format!("esperaba un número, vino {otro:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evalua() {
        assert_eq!(evaluar_num("3/4 + 1/6").unwrap(), Fraccion::new(11, 12));
        assert_eq!(evaluar_num("(2/3) * 12").unwrap(), Fraccion::entero(8));
        assert_eq!(evaluar_num("1 - 2/5 - 1/5").unwrap(), Fraccion::new(2, 5));
        assert_eq!(evaluar("2/3 > 3/5").unwrap(), Valor::Bool(true));
        assert_eq!(evaluar("1/2 = 2/4").unwrap(), Valor::Bool(true));
    }
}
