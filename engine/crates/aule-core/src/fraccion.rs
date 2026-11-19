//! Tipo `Fraccion` exacto (racional con i64), siempre normalizado.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};
use std::str::FromStr;

/// Fracción exacta. Internamente siempre simplificada y con denominador positivo.
/// Para conservar "cómo la escribió el alumno" se usa [`FraccionEscrita`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fraccion {
    num: i64,
    den: i64,
}

pub fn mcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

pub fn mcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        return 0;
    }
    (a / mcd(a, b) * b).abs()
}

impl Fraccion {
    /// Crea una fracción. Panic si `den == 0` (solo se usa con datos validados).
    pub fn new(num: i64, den: i64) -> Self {
        Self::try_new(num, den).expect("denominador cero")
    }

    pub fn try_new(num: i64, den: i64) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let g = mcd(num, den).max(1);
        let s = if den < 0 { -1 } else { 1 };
        Some(Fraccion {
            num: s * num / g,
            den: s * den / g,
        })
    }

    pub fn entero(n: i64) -> Self {
        Fraccion { num: n, den: 1 }
    }

    pub fn num(&self) -> i64 {
        self.num
    }
    pub fn den(&self) -> i64 {
        self.den
    }
    pub fn es_entero(&self) -> bool {
        self.den == 1
    }

    /// Parte entera y resto propio (para números mixtos). Solo para positivos.
    pub fn como_mixto(&self) -> (i64, Fraccion) {
        let e = self.num.div_euclid(self.den);
        (e, Fraccion::new(self.num - e * self.den, self.den))
    }

    pub fn recip(&self) -> Option<Fraccion> {
        Fraccion::try_new(self.den, self.num)
    }

    pub fn a_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl fmt::Display for Fraccion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl Add for Fraccion {
    type Output = Fraccion;
    fn add(self, o: Fraccion) -> Fraccion {
        Fraccion::new(self.num * o.den + o.num * self.den, self.den * o.den)
    }
}
impl Sub for Fraccion {
    type Output = Fraccion;
    fn sub(self, o: Fraccion) -> Fraccion {
        Fraccion::new(self.num * o.den - o.num * self.den, self.den * o.den)
    }
}
impl Mul for Fraccion {
    type Output = Fraccion;
    fn mul(self, o: Fraccion) -> Fraccion {
        Fraccion::new(self.num * o.num, self.den * o.den)
    }
}
impl Div for Fraccion {
    type Output = Fraccion;
    fn div(self, o: Fraccion) -> Fraccion {
        Fraccion::new(self.num * o.den, self.den * o.num)
    }
}
impl Neg for Fraccion {
    type Output = Fraccion;
    fn neg(self) -> Fraccion {
        Fraccion::new(-self.num, self.den)
    }
}
impl PartialOrd for Fraccion {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Fraccion {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.num as i128 * o.den as i128).cmp(&(o.num as i128 * self.den as i128))
    }
}

impl FromStr for Fraccion {
    type Err = String;
    /// Acepta "3/4", "7", "1 1/2" (mixto) y "-2/3".
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        FraccionEscrita::from_str(s).map(|e| e.valor())
    }
}

impl Serialize for Fraccion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Fraccion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Una fracción tal como se escribió: conserva si es mixta, impropia,
/// simplificada o no. Sirve para comparar "por valor y por la forma pedida".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FraccionEscrita {
    pub entero: Option<i64>,
    pub num: i64,
    pub den: i64,
}

impl FraccionEscrita {
    pub fn valor(&self) -> Fraccion {
        let base = Fraccion::new(self.num, self.den);
        match self.entero {
            Some(e) if e < 0 => Fraccion::entero(e) - base,
            Some(e) => Fraccion::entero(e) + base,
            None => base,
        }
    }
    pub fn es_mixto(&self) -> bool {
        self.entero.is_some() && self.num != 0
    }
    pub fn es_entero_escrito(&self) -> bool {
        self.entero.is_none() && self.den == 1
    }
    /// Fracción simple (sin parte entera) en su mínima expresión.
    pub fn esta_simplificada(&self) -> bool {
        mcd(self.num, self.den) == 1
    }
    pub fn es_impropia(&self) -> bool {
        self.entero.is_none() && self.num.abs() >= self.den.abs() && self.den != 1
    }
}

impl fmt::Display for FraccionEscrita {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.entero {
            Some(e) if self.num == 0 => write!(f, "{e}"),
            Some(e) => write!(f, "{e} {}/{}", self.num, self.den),
            None if self.den == 1 => write!(f, "{}", self.num),
            None => write!(f, "{}/{}", self.num, self.den),
        }
    }
}

impl FromStr for FraccionEscrita {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let limpio = s
            .trim()
            .replace(['⁄', '∕', '÷'], "/")
            .replace(" / ", "/")
            .replace("/ ", "/")
            .replace(" /", "/");
        let partes: Vec<&str> = limpio.split_whitespace().collect();
        let entero_de = |t: &str| t.parse::<i64>().map_err(|_| format!("no es número: {t}"));
        let frac_de = |t: &str| -> Result<(i64, i64), String> {
            let (a, b) = t.split_once('/').ok_or("falta /")?;
            let a = entero_de(a)?;
            let b = entero_de(b)?;
            if b == 0 {
                return Err("denominador cero".into());
            }
            Ok((a, b))
        };
        match partes.as_slice() {
            [uno] if uno.contains('/') => {
                let (n, d) = frac_de(uno)?;
                Ok(FraccionEscrita {
                    entero: None,
                    num: n,
                    den: d,
                })
            }
            [uno] => Ok(FraccionEscrita {
                entero: None,
                num: entero_de(uno)?,
                den: 1,
            }),
            [e, f] if f.contains('/') => {
                let (n, d) = frac_de(f)?;
                if n < 0 || d < 0 {
                    return Err("mixto con signo".into());
                }
                Ok(FraccionEscrita {
                    entero: Some(entero_de(e)?),
                    num: n,
                    den: d,
                })
            }
            _ => Err(format!("no reconozco la fracción: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza() {
        assert_eq!(Fraccion::new(2, 4), Fraccion::new(1, 2));
        assert_eq!(Fraccion::new(3, -6), Fraccion::new(-1, 2));
        assert_eq!(
            Fraccion::new(1, 3) + Fraccion::new(1, 6),
            Fraccion::new(1, 2)
        );
        assert_eq!(
            Fraccion::new(3, 4) * Fraccion::entero(20),
            Fraccion::entero(15)
        );
        assert!(Fraccion::new(2, 3) > Fraccion::new(3, 5));
    }

    #[test]
    fn parsea_mixtos() {
        let m: FraccionEscrita = "1 1/2".parse().unwrap();
        assert!(m.es_mixto());
        assert_eq!(m.valor(), Fraccion::new(3, 2));
        let i: FraccionEscrita = "3/2".parse().unwrap();
        assert!(i.es_impropia());
        let s: FraccionEscrita = "4 / 8".parse().unwrap();
        assert!(!s.esta_simplificada());
        assert_eq!("2 3/4".parse::<Fraccion>().unwrap(), Fraccion::new(11, 4));
        assert!("3/0".parse::<Fraccion>().is_err());
    }

    #[test]
    fn mixto() {
        assert_eq!(Fraccion::new(11, 4).como_mixto(), (2, Fraccion::new(3, 4)));
    }
}
