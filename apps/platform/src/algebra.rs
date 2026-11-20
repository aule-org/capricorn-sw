//! The algebra of secondary school, checked exactly: an equation in one
//! unknown of the first or second degree, two linear equations in two
//! unknowns, a polynomial to write expanded or as a product, and the value of
//! an expression for given values.
//!
//! As with arithmetic (see `exercise`), no language model decides anything
//! here. The tutor states the exercise as it would write it on the board
//! ("2x + 3 = 11", "(x + 2)(x - 3)", "x^2 - 5x + 6", "x + y = 10; x - y = 2",
//! "3x^2 - 1 para x = 2"), its answer is worked out with exact fractions, and
//! what the student said ("x vale cuatro", "x al cuadrado menos x menos seis")
//! is read and compared here. An exercise whose answer cannot be written
//! exactly (an irrational root) or found here (a polynomial this does not know
//! how to factor) is not one this checks.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::exercise::{
    Misconception, Rational, Reading, Verdict, cardinal, gcd, parts, plain, says_dont_know,
};

const ZERO: Rational = Rational::whole(0);
const ONE: Rational = Rational::whole(1);
const MINUS_ONE: Rational = Rational::whole(-1);

// Bounds that keep what someone typed or said from turning into a large computation.
const MAX_EXPONENT: u32 = 6;
const MAX_DEGREE: u32 = 12;
const MAX_TERMS: usize = 64;
const MAX_DEPTH: usize = 12;
const MAX_TOKENS: usize = 200;

// Polynomials -----------------------------------------------------------------

/// Unknowns with their exponents, in alphabetical order: x²y is [('x', 2), ('y', 1)].
type Monomial = Vec<(char, u32)>;

/// A polynomial with exact coefficients. No coefficient is zero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Poly(BTreeMap<Monomial, Rational>);

fn multiply(a: &Monomial, b: &Monomial) -> Option<Monomial> {
    let mut exponents: BTreeMap<char, u32> = a.iter().copied().collect();
    for &(unknown, exponent) in b {
        *exponents.entry(unknown).or_insert(0) += exponent;
    }
    (exponents.values().sum::<u32>() <= MAX_DEGREE).then(|| exponents.into_iter().collect())
}

impl Poly {
    fn constant(value: Rational) -> Self {
        let mut terms = BTreeMap::new();
        if value != ZERO {
            terms.insert(Vec::new(), value);
        }
        Self(terms)
    }

    fn unknown(name: char) -> Self {
        Self(BTreeMap::from([(vec![(name, 1)], ONE)]))
    }

    fn plus(&self, other: &Self) -> Option<Self> {
        let mut terms = self.0.clone();
        for (monomial, coefficient) in &other.0 {
            let sum = match terms.get(monomial) {
                Some(existing) => existing.plus(*coefficient)?,
                None => *coefficient,
            };
            if sum == ZERO {
                terms.remove(monomial);
            } else {
                terms.insert(monomial.clone(), sum);
            }
        }
        (terms.len() <= MAX_TERMS).then_some(Self(terms))
    }

    fn scaled(&self, factor: Rational) -> Option<Self> {
        if factor == ZERO {
            return Some(Self::default());
        }
        let terms = self
            .0
            .iter()
            .map(|(monomial, coefficient)| Some((monomial.clone(), coefficient.times(factor)?)))
            .collect::<Option<_>>()?;
        Some(Self(terms))
    }

    fn minus(&self, other: &Self) -> Option<Self> {
        self.plus(&other.scaled(MINUS_ONE)?)
    }

    fn times(&self, other: &Self) -> Option<Self> {
        let mut product = Self::default();
        for (a, x) in &self.0 {
            for (b, y) in &other.0 {
                product = product.plus(&Self(BTreeMap::from([(multiply(a, b)?, x.times(*y)?)])))?;
            }
        }
        Some(product)
    }

    fn power(&self, exponent: u32) -> Option<Self> {
        let mut out = Self::constant(ONE);
        for _ in 0..exponent {
            out = out.times(self)?;
        }
        Some(out)
    }

    fn as_constant(&self) -> Option<Rational> {
        match self.0.len() {
            0 => Some(ZERO),
            1 => self.0.get(&Vec::new()).copied(),
            _ => None,
        }
    }

    fn unknowns(&self) -> BTreeSet<char> {
        self.0.keys().flatten().map(|(unknown, _)| *unknown).collect()
    }

    fn degree(&self) -> u32 {
        self.0.keys().map(|monomial| monomial.iter().map(|(_, exponent)| exponent).sum()).max().unwrap_or(0)
    }

    /// Coefficients by exponent when `unknown` is the only one: [c, b, a] for ax² + bx + c.
    fn coefficients(&self, unknown: char) -> Option<Vec<Rational>> {
        let mut out = vec![ZERO; self.degree() as usize + 1];
        for (monomial, coefficient) in &self.0 {
            match monomial.as_slice() {
                [] => out[0] = *coefficient,
                [(name, exponent)] if *name == unknown => out[*exponent as usize] = *coefficient,
                _ => return None,
            }
        }
        Some(out)
    }

    fn coefficient(&self, monomial: &[(char, u32)]) -> Rational {
        self.0.get(monomial).copied().unwrap_or(ZERO)
    }

    fn at(&self, values: &BTreeMap<char, Rational>) -> Option<Rational> {
        let mut total = ZERO;
        for (monomial, coefficient) in &self.0 {
            let mut term = *coefficient;
            for (unknown, exponent) in monomial {
                for _ in 0..*exponent {
                    term = term.times(*values.get(unknown)?)?;
                }
            }
            total = total.plus(term)?;
        }
        Some(total)
    }

    /// The terms as they are written on a board: highest degree first.
    fn ordered(&self) -> Vec<(&Monomial, Rational)> {
        let unknowns: Vec<char> = self.unknowns().into_iter().collect();
        let mut terms: Vec<(&Monomial, Rational)> = self.0.iter().map(|(m, c)| (m, *c)).collect();
        terms.sort_by_key(|(monomial, _)| {
            let exponents: Vec<u32> = unknowns
                .iter()
                .map(|unknown| monomial.iter().find(|(name, _)| name == unknown).map_or(0, |(_, e)| *e))
                .collect();
            (Reverse(monomial.iter().map(|(_, e)| e).sum::<u32>()), Reverse(exponents))
        });
        terms
    }

    fn expr(&self) -> Expr {
        let mut terms: Vec<Expr> =
            self.ordered().into_iter().map(|(monomial, coefficient)| term(monomial, coefficient)).collect();
        match terms.len() {
            0 => Expr::Number(ZERO),
            1 => terms.remove(0),
            _ => Expr::Sum(terms),
        }
    }

    /// Divided by the unknowns every term shares: 6x²y + 9x is x(6xy + 9).
    fn shared_unknowns(&self) -> Monomial {
        self.unknowns()
            .into_iter()
            .filter_map(|unknown| {
                let lowest = self
                    .0
                    .keys()
                    .map(|monomial| monomial.iter().find(|(name, _)| *name == unknown).map_or(0, |(_, e)| *e))
                    .min()?;
                (lowest > 0).then_some((unknown, lowest))
            })
            .collect()
    }

    fn divided_by(&self, shared: &Monomial) -> Self {
        let terms = self
            .0
            .iter()
            .map(|(monomial, coefficient)| {
                let reduced: Monomial = monomial
                    .iter()
                    .filter_map(|&(unknown, exponent)| {
                        let less = shared.iter().find(|(name, _)| *name == unknown).map_or(0, |(_, e)| *e);
                        (exponent > less).then_some((unknown, exponent - less))
                    })
                    .collect();
                (reduced, *coefficient)
            })
            .collect();
        Self(terms)
    }

    /// The largest whole number dividing every coefficient, when all are whole.
    fn content(&self) -> Option<i64> {
        self.0.values().all(|c| c.is_whole()).then(|| self.0.values().fold(0, |g, c| gcd(g, c.numerator())))
    }
}

/// One term with its coefficient, as written: "3x^2y", "-x", "1/2".
fn term(monomial: &Monomial, coefficient: Rational) -> Expr {
    let size =
        if coefficient < ZERO { coefficient.times(MINUS_ONE).unwrap_or(coefficient) } else { coefficient };
    let mut factors = Vec::new();
    if size != ONE || monomial.is_empty() {
        factors.push(Expr::Number(size));
    }
    for &(unknown, exponent) in monomial {
        factors.push(match exponent {
            1 => Expr::Unknown(unknown),
            _ => Expr::Power(Box::new(Expr::Unknown(unknown)), exponent),
        });
    }
    let positive = if factors.len() == 1 { factors.remove(0) } else { Expr::Product(factors) };
    if coefficient < ZERO { Expr::Negative(Box::new(positive)) } else { positive }
}

/// √value when it is a fraction.
fn exact_root(value: Rational) -> Option<Rational> {
    let root = |n: i64| {
        let guess = (n as f64).sqrt().round() as i64;
        (guess.saturating_sub(1)..=guess.saturating_add(1)).find(|r| *r >= 0 && r.checked_mul(*r) == Some(n))
    };
    if value < ZERO {
        return None;
    }
    Rational::new(root(value.numerator())?, root(value.denominator())?)
}

/// The rational roots of ax² + bx + c (coefficients [c, b, a]), smallest
/// first, each as often as it is a root. None when they are not rational.
fn quadratic_roots(coefficients: &[Rational]) -> Option<Vec<Rational>> {
    let [c, b, a] = coefficients else { return None };
    let discriminant = b.times(*b)?.minus(Rational::whole(4).times(*a)?.times(*c)?)?;
    let root = exact_root(discriminant)?;
    let twice = Rational::whole(2).times(*a)?;
    let minus_b = b.times(MINUS_ONE)?;
    let mut roots = vec![minus_b.minus(root)?.over(twice)?, minus_b.plus(root)?.over(twice)?];
    roots.sort();
    Some(roots)
}

/// Whether a factor cannot be split further the way school algebra splits:
/// nothing in common, and no rational root when it is in one unknown.
fn irreducible(poly: &Poly) -> bool {
    if poly.0.len() < 2 {
        return true;
    }
    if !poly.shared_unknowns().is_empty() || poly.content().is_some_and(|g| g > 1) {
        return false;
    }
    let unknowns = poly.unknowns();
    let Some(&unknown) = unknowns.first().filter(|_| unknowns.len() == 1) else { return true };
    let Some(coefficients) = poly.coefficients(unknown) else { return true };
    match coefficients.len() {
        3 => quadratic_roots(&coefficients).is_none(),
        4.. => !has_rational_root(&coefficients),
        _ => true,
    }
}

/// The rational root test, for whole coefficients that are not too large.
fn has_rational_root(coefficients: &[Rational]) -> bool {
    const LIMIT: i64 = 10_000;
    let whole: Option<Vec<i64>> = coefficients.iter().map(|c| c.is_whole().then(|| c.numerator())).collect();
    let Some(whole) = whole else { return false };
    let (Some(&last), Some(&first)) = (whole.last(), whole.first()) else { return false };
    if first == 0 {
        return true;
    }
    if first.abs() > LIMIT || last.abs() > LIMIT {
        return false;
    }
    let divisors = |n: i64| (1..=n.abs()).filter(move |d| n % d == 0);
    for p in divisors(first) {
        for q in divisors(last) {
            for sign in [1, -1] {
                let Some(candidate) = Rational::new(sign * p, q) else { continue };
                let mut value = ZERO;
                for c in coefficients.iter().rev() {
                    value = match value.times(candidate).and_then(|v| v.plus(*c)) {
                        Some(v) => v,
                        None => break,
                    };
                }
                if value == ZERO {
                    return true;
                }
            }
        }
    }
    false
}

/// The polynomial written as a product, when there is something to take out
/// or it is ax² + bx + c with rational roots. None when this cannot factor it.
fn factorization(poly: &Poly) -> Option<Expr> {
    if poly.0.len() < 2 {
        return None;
    }
    let leading = poly.ordered().first()?.1;
    let mut constant = match poly.content() {
        Some(g) => Rational::whole(if leading < ZERO { -g } else { g }),
        None => ONE,
    };
    let shared = poly.shared_unknowns();
    let rest = poly.scaled(ONE.over(constant)?)?.divided_by(&shared);
    let mut factors: Vec<Expr> = shared
        .iter()
        .map(|&(unknown, exponent)| match exponent {
            1 => Expr::Unknown(unknown),
            _ => Expr::Power(Box::new(Expr::Unknown(unknown)), exponent),
        })
        .collect();
    let unknowns = rest.unknowns();
    let quadratic = match (unknowns.first(), unknowns.len()) {
        (Some(&unknown), 1) => rest.coefficients(unknown).filter(|c| c.len() == 3).map(|c| (unknown, c)),
        _ => None,
    };
    if let Some((unknown, coefficients)) = quadratic
        && let Some(roots) = quadratic_roots(&coefficients)
    {
        // A root p/q is the factor (qx - p); what is left of the leading coefficient goes in front.
        let mut lead = coefficients[2];
        let mut linear = Vec::new();
        for root in &roots {
            let q = Rational::whole(root.denominator());
            lead = lead.over(q)?;
            linear.push(
                Poly::unknown(unknown)
                    .scaled(q)?
                    .minus(&Poly::constant(Rational::whole(root.numerator())))?,
            );
        }
        constant = constant.times(lead)?;
        if roots[0] == roots[1] {
            factors.push(Expr::Power(Box::new(linear[0].expr()), 2));
        } else {
            factors.extend(linear.iter().map(Poly::expr));
        }
    } else if rest.as_constant() != Some(ONE) {
        factors.push(rest.expr());
    }
    let size = if constant < ZERO { constant.times(MINUS_ONE)? } else { constant };
    let repeated = factors
        .iter()
        .filter(|factor| matches!(factor, Expr::Power(base, _) if matches!(**base, Expr::Sum(_))))
        .count();
    let taken = factors.len() + repeated + usize::from(size != ONE);
    if taken < 2 {
        return None;
    }
    if size != ONE {
        factors.insert(0, Expr::Number(size));
    }
    let product = if factors.len() == 1 { factors.remove(0) } else { Expr::Product(factors) };
    Some(if constant < ZERO { Expr::Negative(Box::new(product)) } else { product })
}

// Expressions -----------------------------------------------------------------

/// An expression as it was written, which says whether it is expanded or a product.
#[derive(Clone, Debug, PartialEq)]
enum Expr {
    Number(Rational),
    Unknown(char),
    /// Terms added; one taken away is a `Negative` term.
    Sum(Vec<Expr>),
    Product(Vec<Expr>),
    Quotient(Box<Expr>, Box<Expr>),
    Power(Box<Expr>, u32),
    Negative(Box<Expr>),
}

impl Expr {
    fn poly(&self) -> Option<Poly> {
        match self {
            Self::Number(value) => Some(Poly::constant(*value)),
            Self::Unknown(name) => Some(Poly::unknown(*name)),
            Self::Sum(terms) => terms.iter().try_fold(Poly::default(), |sum, term| sum.plus(&term.poly()?)),
            Self::Product(factors) => {
                factors.iter().try_fold(Poly::constant(ONE), |product, factor| product.times(&factor.poly()?))
            }
            Self::Quotient(top, bottom) => {
                let bottom = bottom.poly()?.as_constant().filter(|value| *value != ZERO)?;
                top.poly()?.scaled(ONE.over(bottom)?)
            }
            Self::Power(base, exponent) => base.poly()?.power(*exponent),
            Self::Negative(inner) => inner.poly()?.scaled(MINUS_ONE),
        }
    }
}

fn number(f: &mut fmt::Formatter<'_>, value: Rational) -> fmt::Result {
    if value.is_whole() {
        write!(f, "{}", value.numerator())
    } else {
        write!(f, "{}/{}", value.numerator(), value.denominator())
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn grouped(f: &mut fmt::Formatter<'_>, e: &Expr, when: bool) -> fmt::Result {
            if when { write!(f, "({e})") } else { write!(f, "{e}") }
        }
        match self {
            Self::Number(value) => number(f, *value),
            Self::Unknown(name) => write!(f, "{name}"),
            Self::Sum(terms) => {
                for (i, term) in terms.iter().enumerate() {
                    match (i, term) {
                        (0, term) => write!(f, "{term}")?,
                        (_, Self::Negative(inner)) => {
                            write!(f, " - ")?;
                            grouped(f, inner, matches!(**inner, Self::Sum(_)))?;
                        }
                        (_, term) => write!(f, " + {term}")?,
                    }
                }
                Ok(())
            }
            Self::Product(factors) => {
                for (i, factor) in factors.iter().enumerate() {
                    if i > 0 && matches!(factor, Self::Number(_)) {
                        write!(f, "·")?;
                    }
                    let fraction = matches!(factor, Self::Number(value) if !value.is_whole());
                    grouped(
                        f,
                        factor,
                        fraction || matches!(factor, Self::Sum(_) | Self::Negative(_) | Self::Quotient(..)),
                    )?;
                }
                Ok(())
            }
            Self::Quotient(top, bottom) => {
                let compound = |e: &Expr| {
                    matches!(e, Self::Sum(_) | Self::Product(_) | Self::Negative(_) | Self::Quotient(..))
                };
                grouped(f, top, compound(top))?;
                write!(f, "/")?;
                grouped(f, bottom, compound(bottom))
            }
            Self::Power(base, exponent) => {
                let plain = matches!(**base, Self::Unknown(_))
                    || matches!(**base, Self::Number(value) if value.is_whole() && value >= ZERO);
                grouped(f, base, !plain)?;
                write!(f, "^{exponent}")
            }
            Self::Negative(inner) => {
                write!(f, "-")?;
                grouped(f, inner, matches!(**inner, Self::Sum(_)))
            }
        }
    }
}

/// The unknowns of a term written as one number times unknowns, each once:
/// "3x^2y". None for anything else.
fn monomial_of(term: &Expr) -> Option<Monomial> {
    let factors: Vec<&Expr> = match term {
        Expr::Product(factors) => factors.iter().collect(),
        other => vec![other],
    };
    let mut numbers = 0;
    let mut exponents: BTreeMap<char, u32> = BTreeMap::new();
    for factor in factors {
        let (unknown, exponent) = match factor {
            Expr::Number(value) if *value != ZERO => {
                numbers += 1;
                continue;
            }
            Expr::Quotient(top, bottom)
                if matches!(**top, Expr::Number(_)) && matches!(**bottom, Expr::Number(_)) =>
            {
                numbers += 1;
                continue;
            }
            Expr::Unknown(name) => (*name, 1),
            Expr::Power(base, exponent) => match **base {
                Expr::Unknown(name) => (name, *exponent),
                _ => return None,
            },
            _ => return None,
        };
        if exponents.insert(unknown, exponent).is_some() {
            return None;
        }
    }
    (numbers <= 1).then(|| exponents.into_iter().collect())
}

/// Written expanded: a sum of terms, each a number times unknowns, no two alike.
fn is_expanded(expr: &Expr) -> bool {
    let terms: Vec<&Expr> = match expr {
        Expr::Sum(terms) => terms.iter().collect(),
        other => vec![other],
    };
    let mut seen = BTreeSet::new();
    terms.into_iter().all(|term| {
        let term = match term {
            Expr::Negative(inner) => inner.as_ref(),
            term => term,
        };
        monomial_of(term).is_some_and(|monomial| seen.insert(monomial))
    })
}

/// Written as a product whose factors cannot be split further.
fn is_factored(expr: &Expr) -> bool {
    let expr = match expr {
        Expr::Negative(inner) => inner.as_ref(),
        expr => expr,
    };
    let factors: Vec<&Expr> = match expr {
        Expr::Product(factors) => factors.iter().collect(),
        Expr::Power(base, exponent) if *exponent >= 2 => vec![base.as_ref(), base.as_ref()],
        _ => return false,
    };
    let (mut variable, mut constant) = (0, false);
    for factor in factors {
        let base = match factor {
            Expr::Power(base, _) => base.as_ref(),
            factor => factor,
        };
        let Some(poly) = base.poly() else { return false };
        match poly.as_constant() {
            Some(value) => constant |= value != ONE && value != MINUS_ONE,
            None if irreducible(&poly) => variable += 1,
            None => return false,
        }
    }
    variable >= 2 || (variable == 1 && constant)
}

// Reading ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Number(Rational),
    Unknown(char),
    /// A single letter that is also a word ("y", "a"): an unknown or not, by its neighbours.
    Letter(char),
    Plus,
    Minus,
    Times,
    Over,
    Caret,
    Open,
    Close,
    /// "=", "es", "vale", "da": what follows is a value.
    Equals,
    /// "o": another solution follows.
    Or,
    And,
    /// ";", ",", "para": another part of the exercise.
    Next,
    Word,
}

/// Spanish and English words of algebra said aloud. None for a word that says nothing here.
fn word(word: &str, last: Option<Token>) -> Option<Vec<Token>> {
    use Token::{And, Caret, Close, Equals, Minus, Next, Number, Open, Or, Over, Plus, Times, Unknown};
    let after_operator = matches!(last, Some(Times | Over | Caret | Equals));
    let squared = |n: i64| {
        if last == Some(Caret) {
            vec![Number(Rational::whole(n))]
        } else {
            vec![Caret, Number(Rational::whole(n))]
        }
    };
    Some(match word {
        "mas" | "plus" => vec![Plus],
        "menos" | "minus" | "negativo" | "negative" => vec![Minus],
        "por" | "entre" | "by" if after_operator => return None,
        "por" | "times" | "veces" | "multiplicado" | "multiplied" => vec![Times],
        "entre" | "sobre" | "dividido" | "over" | "divided" => vec![Over],
        "es" | "igual" | "son" | "vale" | "valen" | "da" | "dan" | "dio" | "sale" | "salio" | "queda"
        | "quedo" | "equals" | "equal" | "is" | "are" | "gives" => {
            if last == Some(Equals) {
                return None;
            }
            vec![Equals]
        }
        "a" | "al" | "la" | "el" | "to" | "the" | "of" if matches!(last, Some(Caret | Equals)) => {
            return None;
        }
        "a" => vec![Token::Letter('a')],
        "y" => vec![Token::Letter('y')],
        "o" | "u" | "or" => vec![Or],
        "e" | "and" => vec![And],
        "cuadrado" | "squared" => squared(2),
        "cubo" | "cubed" => squared(3),
        "elevado" | "elevada" | "power" | "potencia" => vec![Caret],
        "abre" | "abro" | "abrir" | "open" => vec![Open],
        "cierra" | "cierro" | "cerrar" | "close" => vec![Close],
        "equis" => vec![Unknown('x')],
        "ye" => vec![Unknown('y')],
        "zeta" | "zed" | "zee" => vec![Unknown('z')],
        "para" | "cuando" | "si" | "con" | "with" | "when" | "for" | "where" | "donde" => vec![Next],
        "parentesis" | "bracket" | "brackets" | "corchete" | "al" | "raised" => return None,
        _ => {
            if let Some(n) = cardinal(word).filter(|_| word.chars().count() > 1) {
                return Some(vec![Number(Rational::whole(n))]);
            }
            if let Some(d) = parts(word) {
                return Some(vec![Token::Over, Number(Rational::whole(d))]);
            }
            let letters: Vec<char> = word.chars().collect();
            if letters.len() <= 3 && letters.iter().all(|c| "abcmnpqrstxyz".contains(*c)) {
                return Some(letters.into_iter().map(Token::Letter).collect());
            }
            vec![Token::Word]
        }
    })
}

/// The text as tokens, each marked when nothing separated it from the one before.
fn lex(text: &str) -> Vec<(Token, bool)> {
    let text = plain(text).replace('²', "^2").replace('³', "^3");
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(Token, bool)> = Vec::new();
    let mut glued = false;
    let mut i = 0;
    while i < chars.len() && out.len() < MAX_TOKENS {
        let c = chars[i];
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            // A decimal point or comma only counts between digits.
            if i + 1 < chars.len() && matches!(chars[i], '.' | ',') && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let digits: String = chars[start..i].iter().map(|c| if *c == ',' { '.' } else { *c }).collect();
            out.push((decimal(&digits).map_or(Token::Word, Token::Number), glued));
            glued = true;
            continue;
        }
        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            if let Some(tokens) = word(&text, out.last().map(|(token, _)| *token)) {
                for (n, token) in tokens.into_iter().enumerate() {
                    out.push((token, glued || n > 0));
                }
            }
            glued = true;
            continue;
        }
        let token = match c {
            '+' => Some(Token::Plus),
            '-' => Some(Token::Minus),
            '*' => Some(Token::Times),
            '/' => Some(Token::Over),
            // "Resuelve: ..." is not a division.
            ':' if matches!(
                out.last(),
                Some((Token::Number(_) | Token::Letter(_) | Token::Unknown(_) | Token::Close, _))
            ) =>
            {
                Some(Token::Over)
            }
            '^' => Some(Token::Caret),
            '(' | '[' => Some(Token::Open),
            ')' | ']' => Some(Token::Close),
            '=' => Some(Token::Equals),
            ';' | ',' => Some(Token::Next),
            _ => None,
        };
        match token {
            Some(token) => {
                out.push((token, glued));
                glued = true;
            }
            None => glued = false,
        }
        i += 1;
    }
    out
}

fn decimal(digits: &str) -> Option<Rational> {
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if fraction.len() > 6 || whole.len() > 12 {
        return None;
    }
    let scale = 10_i64.checked_pow(fraction.len() as u32)?;
    let whole: i64 = whole.parse().ok()?;
    let fraction: i64 = if fraction.is_empty() { 0 } else { fraction.parse().ok()? };
    Rational::new(whole.checked_mul(scale)?.checked_add(fraction)?, scale)
}

/// Settles the letters that are also words, and joins "treinta y cinco".
/// `unknowns` is None for the tutor's own exercise, where every letter is an
/// unknown; for the student's words it is the exercise's unknowns.
fn resolve(tokens: Vec<(Token, bool)>, unknowns: Option<&BTreeSet<char>>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for (i, &(token, glued)) in tokens.iter().enumerate() {
        let Token::Letter(letter) = token else {
            out.push(token);
            continue;
        };
        let next = tokens.get(i + 1).map(|(token, _)| *token);
        let previous = out.last().copied();
        let known = unknowns.is_none_or(|unknowns| unknowns.contains(&letter));
        let reads_as_unknown = match (unknowns, letter) {
            // "para a = 4 y b = -1": between a value and another letter, "y" joins.
            (None, 'y') => {
                glued
                    || !(matches!(previous, Some(Token::Number(_) | Token::Close))
                        && matches!(next, Some(Token::Letter(_) | Token::Unknown(_))))
            }
            (None, _) => true,
            (Some(_), 'y' | 'a') => {
                matches!(next, Some(Token::Equals | Token::Caret | Token::Times | Token::Over | Token::Close))
                    || (glued
                        && matches!(
                            previous,
                            Some(Token::Number(_) | Token::Close | Token::Unknown(_) | Token::Caret)
                        ))
            }
            (Some(_), _) => true,
        };
        if known && reads_as_unknown {
            // "x=6 y=4": an unknown before "=" right after a value starts another one.
            if unknowns.is_some()
                && !glued
                && next == Some(Token::Equals)
                && matches!(previous, Some(Token::Number(_) | Token::Close))
            {
                out.push(Token::Next);
            }
            out.push(Token::Unknown(letter));
        } else if letter == 'y' {
            out.push(Token::And);
        } else if letter != 'a' {
            out.push(Token::Word);
        }
    }
    // "treinta y cinco"
    let mut joined: Vec<Token> = Vec::new();
    for token in out {
        let total = match (token, joined.as_slice()) {
            (Token::Number(units), [.., Token::Number(tens), Token::And])
                if tens.is_whole()
                    && (30..=90).contains(&tens.numerator())
                    && tens.numerator() % 10 == 0
                    && units.is_whole()
                    && (1..=9).contains(&units.numerator()) =>
            {
                tens.plus(units)
            }
            _ => None,
        };
        if let Some(total) = total {
            joined.pop();
            joined.pop();
            joined.push(Token::Number(total));
            continue;
        }
        joined.push(token);
    }
    joined
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn sum(&mut self) -> Option<Expr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let mut terms = Vec::new();
        let mut negative = match self.peek() {
            Some(Token::Minus) => {
                self.at += 1;
                true
            }
            Some(Token::Plus) => {
                self.at += 1;
                false
            }
            _ => false,
        };
        loop {
            let term = self.product()?;
            terms.push(if negative { Expr::Negative(Box::new(term)) } else { term });
            negative = match self.peek() {
                Some(Token::Plus) => false,
                Some(Token::Minus) => true,
                _ => break,
            };
            self.at += 1;
        }
        self.depth -= 1;
        Some(if terms.len() == 1 { terms.remove(0) } else { Expr::Sum(terms) })
    }

    fn product(&mut self) -> Option<Expr> {
        let mut factors = vec![self.power()?];
        loop {
            match self.peek() {
                Some(Token::Times) => {
                    self.at += 1;
                    factors.push(self.power()?);
                }
                Some(Token::Over) => {
                    self.at += 1;
                    let bottom = self.power()?;
                    let top = if factors.len() == 1 { factors.remove(0) } else { Expr::Product(factors) };
                    factors = vec![Expr::Quotient(Box::new(top), Box::new(bottom))];
                }
                // Written side by side: 2x, x(x + 1), (x + 1)(x - 1).
                Some(Token::Unknown(_) | Token::Open) => factors.push(self.power()?),
                _ => break,
            }
        }
        let mut flat = Vec::new();
        for factor in factors {
            match factor {
                Expr::Product(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        Some(if flat.len() == 1 { flat.remove(0) } else { Expr::Product(flat) })
    }

    fn power(&mut self) -> Option<Expr> {
        let base = self.atom()?;
        if self.peek() != Some(Token::Caret) {
            return Some(base);
        }
        self.at += 1;
        let Some(Token::Number(exponent)) = self.peek() else { return None };
        self.at += 1;
        let exponent =
            u32::try_from(exponent.numerator()).ok().filter(|e| exponent.is_whole() && *e <= MAX_EXPONENT)?;
        Some(Expr::Power(Box::new(base), exponent))
    }

    fn atom(&mut self) -> Option<Expr> {
        let token = self.peek()?;
        self.at += 1;
        match token {
            Token::Number(value) => Some(Expr::Number(value)),
            Token::Unknown(name) => Some(Expr::Unknown(name)),
            Token::Minus => Some(Expr::Negative(Box::new(self.power()?))),
            Token::Open => {
                let inner = self.sum()?;
                (self.peek() == Some(Token::Close)).then(|| self.at += 1)?;
                Some(inner)
            }
            _ => None,
        }
    }
}

/// The tokens as one expression, all of them used. None otherwise.
fn expression(tokens: &[Token]) -> Option<Expr> {
    let mut parser = Parser { tokens, at: 0, depth: 0 };
    let expr = parser.sum()?;
    (parser.at == tokens.len()).then_some(expr)
}

/// Read aloud, "x menos dos por x menos tres" means (x - 2)(x - 3): each
/// "por" between sums splits factors. The second way of reading a stretch.
fn split_at_times(tokens: &[Token]) -> Option<Expr> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Open => depth += 1,
            Token::Close => depth -= 1,
            Token::Times if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    if parts.len() < 2 {
        return None;
    }
    let factors: Option<Vec<Expr>> = parts.into_iter().map(expression).collect();
    Some(Expr::Product(factors?))
}

/// A number, signed, possibly a fraction: "-3/2", "+ 4", "menos tres medios".
fn signed(tokens: &[Token], at: usize) -> Option<(Rational, usize)> {
    let (sign, start) = match tokens.get(at)? {
        Token::Minus => (MINUS_ONE, at + 1),
        Token::Plus => (ONE, at + 1),
        _ => (ONE, at),
    };
    let Token::Number(value) = *tokens.get(start)? else { return None };
    let (value, end) = match (tokens.get(start + 1), tokens.get(start + 2)) {
        (Some(Token::Over), Some(Token::Number(bottom))) if *bottom != ZERO => {
            (value.over(*bottom)?, start + 3)
        }
        _ => (value, start + 1),
    };
    // "2x" is not the number 2.
    if matches!(tokens.get(end), Some(Token::Unknown(_) | Token::Caret | Token::Open)) {
        return None;
    }
    Some((value.times(sign)?, end - at))
}

/// The values someone gave: named ("x = 4", "x = 2 o -5") and the others,
/// with those after the last "=" or "es" apart.
#[derive(Default)]
struct Values {
    named: Vec<(char, Rational)>,
    loose: Vec<Rational>,
    after_marker: Option<Vec<Rational>>,
}

fn values(tokens: &[Token]) -> Values {
    let mut found = Values::default();
    let mut naming: Option<char> = None;
    let mut i = 0;
    while i < tokens.len() {
        if let (Token::Unknown(name), Some(Token::Equals)) = (tokens[i], tokens.get(i + 1))
            && let Some((value, used)) = signed(tokens, i + 2)
        {
            found.named.push((name, value));
            naming = Some(name);
            i += 2 + used;
            continue;
        }
        // "x = 2 o -5": a value right after "o" or "y" is another value of the same unknown.
        if let (Some(name), Token::Or | Token::And | Token::Next) = (naming, tokens[i])
            && let Some((value, used)) = signed(tokens, i + 1)
            && !matches!(tokens.get(i + 1 + used), Some(Token::Equals))
        {
            found.named.push((name, value));
            i += 1 + used;
            continue;
        }
        if tokens[i] == Token::Equals {
            found.after_marker = Some(Vec::new());
            naming = None;
            i += 1;
            continue;
        }
        if let Some((value, used)) = signed(tokens, i) {
            found.loose.push(value);
            if let Some(after) = found.after_marker.as_mut() {
                after.push(value);
            }
            i += used;
            continue;
        }
        if !matches!(tokens[i], Token::Or | Token::And | Token::Next) {
            naming = None;
        }
        i += 1;
    }
    found
}

impl Values {
    /// The last `count` values that were not named, preferring those after the last marker.
    fn last(&self, count: usize) -> Vec<Rational> {
        let pool = match &self.after_marker {
            Some(after) if !after.is_empty() => after,
            _ => &self.loose,
        };
        pool[pool.len().saturating_sub(count)..].to_vec()
    }

    fn of(&self, unknown: char) -> Vec<Rational> {
        self.named.iter().filter(|(name, _)| *name == unknown).map(|(_, value)| *value).collect()
    }
}

// Exercises -------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Expected {
    /// The value of an expression for given values.
    Number(Rational),
    /// The solutions of an equation in one unknown, smallest first.
    Roots { unknown: char, roots: Vec<Rational> },
    /// The solution of two linear equations in two unknowns.
    Pair { unknowns: [char; 2], values: [Rational; 2] },
    /// A polynomial to write expanded, like terms together.
    Expanded(Poly),
    /// A polynomial to write as a product; how it factors, to show it.
    Factored { poly: Poly, shown: String },
}

/// How an algebra exercise asks for its answer to be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Any,
    Expanded,
    Factored,
}

/// An algebra exercise the tutor stated, with its answer worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    expected: Expected,
    unknowns: BTreeSet<char>,
    /// Wrong answers with a name, worked out from how the exercise is written.
    wrong_values: Vec<(Rational, Misconception)>,
    wrong_polys: Vec<(Poly, Misconception)>,
}

fn show(value: Rational) -> String {
    if value.is_whole() {
        value.numerator().to_string()
    } else {
        format!("{}/{}", value.numerator(), value.denominator())
    }
}

impl Problem {
    /// Reads an exercise such as "2x + 3 = 11", "x^2 - 5x + 6 = 0",
    /// "x + y = 10; x - y = 2", "(x + 2)(x - 3)", "x^2 - 5x + 6" (to factor:
    /// it is already expanded) or "3x^2 - 1 para x = 2". None when it is not
    /// algebra this can work out exactly.
    pub fn parse(text: &str) -> Option<Self> {
        let tokens = resolve(lex(text), None);
        let mut alone = Vec::new();
        let mut equations = Vec::new();
        for segment in tokens.split(|token| matches!(token, Token::Next | Token::And)) {
            // "Resuelve: 2x + 3 = 11": words before the mathematics are an instruction.
            let start = segment.iter().position(|token| *token != Token::Word).unwrap_or(segment.len());
            let segment = &segment[start..];
            if segment.is_empty() {
                continue;
            }
            let sides: Vec<&[Token]> = segment.split(|token| *token == Token::Equals).collect();
            match sides.as_slice() {
                [single] => alone.push(expression(single)?),
                [left, right] => equations.push((expression(left)?, expression(right)?)),
                _ => return None,
            }
        }
        match (alone.as_slice(), equations.as_slice()) {
            ([expr], []) => Self::polynomial(expr),
            ([expr], given) => Self::value(expr, given),
            ([], [(left, right)]) => Self::equation(left, right),
            ([], [first, second]) => Self::system(first, second),
            _ => None,
        }
    }

    fn polynomial(expr: &Expr) -> Option<Self> {
        let poly = expr.poly()?;
        let unknowns = poly.unknowns();
        if unknowns.is_empty() {
            return None;
        }
        // Already expanded: what is left to do with it is to factor it.
        let expected = if is_expanded(expr) {
            Expected::Factored { shown: factorization(&poly)?.to_string(), poly }
        } else {
            Expected::Expanded(poly)
        };
        let mut wrong_polys = Vec::new();
        // (a + b)² written a² + b², and (a + b)(c + d) written ac + bd.
        let forgotten = match expr {
            Expr::Power(base, 2) => match base.as_ref() {
                Expr::Sum(terms) => {
                    terms.iter().try_fold(Poly::default(), |sum, term| sum.plus(&term.poly()?.power(2)?))
                }
                _ => None,
            },
            Expr::Product(factors) => match factors.as_slice() {
                [Expr::Sum(first), Expr::Sum(second)] if first.len() == 2 && second.len() == 2 => first[0]
                    .poly()?
                    .times(&second[0].poly()?)?
                    .plus(&first[1].poly()?.times(&second[1].poly()?)?),
                _ => None,
            },
            _ => None,
        };
        if let (Some(forgotten), Expected::Expanded(right)) = (forgotten, &expected)
            && forgotten != *right
        {
            wrong_polys.push((forgotten, Misconception::ForgotCrossTerms));
        }
        Some(Self { expected, unknowns, wrong_values: Vec::new(), wrong_polys })
    }

    fn value(expr: &Expr, given: &[(Expr, Expr)]) -> Option<Self> {
        let mut values = BTreeMap::new();
        for (left, right) in given {
            let Expr::Unknown(name) = left else { return None };
            values.insert(*name, right.poly()?.as_constant()?);
        }
        let poly = expr.poly()?;
        let unknowns = poly.unknowns();
        if unknowns.is_empty() || !unknowns.iter().all(|name| values.contains_key(name)) {
            return None;
        }
        let expected = Expected::Number(poly.at(&values)?);
        Some(Self { expected, unknowns, wrong_values: Vec::new(), wrong_polys: Vec::new() })
    }

    fn equation(left: &Expr, right: &Expr) -> Option<Self> {
        let (left, right) = (left.poly()?, right.poly()?);
        let difference = left.minus(&right)?;
        let unknowns = difference.unknowns();
        let &unknown = unknowns.first().filter(|_| unknowns.len() == 1)?;
        let coefficients = difference.coefficients(unknown)?;
        let roots = match coefficients.as_slice() {
            [c, b] => vec![c.times(MINUS_ONE)?.over(*b)?],
            [_, _, _] => {
                let mut roots = quadratic_roots(&coefficients)?;
                roots.dedup();
                roots
            }
            _ => return None,
        };
        // a1·x + b1 = a2·x + b2: the slips of moving terms across, from how it is written.
        let mut wrong_values = Vec::new();
        let linear = |side: &Poly| {
            let mut coefficients = side.coefficients(unknown)?;
            coefficients.resize(2.max(coefficients.len()), ZERO);
            (coefficients.len() == 2).then(|| (coefficients[1], coefficients[0]))
        };
        if let (Some((a1, b1)), Some((a2, b2))) = (linear(&left), linear(&right)) {
            let (a, b) = (a1.minus(a2)?, b2.minus(b1)?);
            let mut add = |value: Option<Rational>, name| {
                if let Some(value) = value
                    && !roots.contains(&value)
                    && !wrong_values.iter().any(|(known, _)| *known == value)
                {
                    wrong_values.push((value, name));
                }
            };
            if b1 != ZERO {
                add(b2.plus(b1).and_then(|top| top.over(a)), Misconception::TransposedSign);
            }
            if b2 != ZERO {
                add(
                    b2.plus(b1).and_then(|top| top.times(MINUS_ONE)).and_then(|top| top.over(a)),
                    Misconception::TransposedSign,
                );
            }
            if a2 != ZERO {
                add(a1.plus(a2).and_then(|bottom| b.over(bottom)), Misconception::TransposedSign);
            }
            add(b.times(a), Misconception::MultipliedInstead);
            if b != ZERO {
                add(a.over(b), Misconception::DividedBackwards);
            }
        }
        let expected = Expected::Roots { unknown, roots };
        Some(Self { expected, unknowns, wrong_values, wrong_polys: Vec::new() })
    }

    fn system(first: &(Expr, Expr), second: &(Expr, Expr)) -> Option<Self> {
        let first = first.0.poly()?.minus(&first.1.poly()?)?;
        let second = second.0.poly()?.minus(&second.1.poly()?)?;
        let unknowns: BTreeSet<char> = first.unknowns().union(&second.unknowns()).copied().collect();
        let [x, y]: [char; 2] = unknowns.iter().copied().collect::<Vec<_>>().try_into().ok()?;
        if first.degree() != 1 || second.degree() != 1 {
            return None;
        }
        // a·x + b·y + c = 0, twice.
        let sides = |p: &Poly| (p.coefficient(&[(x, 1)]), p.coefficient(&[(y, 1)]), p.coefficient(&[]));
        let ((a1, b1, c1), (a2, b2, c2)) = (sides(&first), sides(&second));
        let determinant = a1.times(b2)?.minus(a2.times(b1)?)?;
        if determinant == ZERO {
            return None;
        }
        let x_value = b1.times(c2)?.minus(b2.times(c1)?)?.over(determinant)?;
        let y_value = a2.times(c1)?.minus(a1.times(c2)?)?.over(determinant)?;
        let expected = Expected::Pair { unknowns: [x, y], values: [x_value, y_value] };
        Some(Self { expected, unknowns, wrong_values: Vec::new(), wrong_polys: Vec::new() })
    }

    /// How the answer is to be written.
    pub fn shape(&self) -> Shape {
        match self.expected {
            Expected::Expanded(_) => Shape::Expanded,
            Expected::Factored { .. } => Shape::Factored,
            _ => Shape::Any,
        }
    }

    /// The answer as it is kept and, when it has to be, shown.
    pub fn answer(&self) -> String {
        match &self.expected {
            Expected::Number(value) => show(*value),
            Expected::Roots { unknown, roots } => {
                roots.iter().map(|root| format!("{unknown} = {}", show(*root))).collect::<Vec<_>>().join(", ")
            }
            Expected::Pair { unknowns, values } => {
                format!("{} = {}, {} = {}", unknowns[0], show(values[0]), unknowns[1], show(values[1]))
            }
            Expected::Expanded(poly) => poly.expr().to_string(),
            Expected::Factored { shown, .. } => shown.clone(),
        }
    }

    /// Judges what the student said against the exercise.
    pub fn check(&self, said: &str) -> Reading {
        let tokens = resolve(lex(said), Some(&self.unknowns));
        let reading = match &self.expected {
            Expected::Number(value) => self.number(&tokens, *value),
            Expected::Roots { unknown, roots } => self.roots(&tokens, *unknown, roots),
            Expected::Pair { unknowns, values } => self.pair(&tokens, *unknowns, *values),
            Expected::Expanded(poly) | Expected::Factored { poly, .. } => self.written(&tokens, poly),
        };
        reading.unwrap_or_else(|| Reading {
            read: None,
            verdict: if says_dont_know(said) { Verdict::DontKnow } else { Verdict::Unreadable },
            misconception: None,
        })
    }

    /// Whether a text of the tutor's gives the answer away. Stating the
    /// exercise again does not.
    pub fn is_given_away_by(&self, said: &str) -> bool {
        self.check(said).verdict == Verdict::Correct
    }

    fn number(&self, tokens: &[Token], expected: Rational) -> Option<Reading> {
        let found = values(tokens);
        let &given = found.last(1).first()?;
        let verdict = if given == expected { Verdict::Correct } else { Verdict::Incorrect };
        Some(Reading { read: Some(show(given)), verdict, misconception: None })
    }

    fn roots(&self, tokens: &[Token], unknown: char, roots: &[Rational]) -> Option<Reading> {
        let found = values(tokens);
        let named = found.of(unknown);
        let given: BTreeSet<Rational> =
            if named.is_empty() { found.last(roots.len()) } else { named }.into_iter().collect();
        if given.is_empty() {
            return None;
        }
        let expected: BTreeSet<Rational> = roots.iter().copied().collect();
        let read =
            given.iter().map(|value| format!("{unknown} = {}", show(*value))).collect::<Vec<_>>().join(", ");
        let (verdict, misconception) = if given == expected {
            (Verdict::Correct, None)
        } else if given.is_subset(&expected) {
            (Verdict::Incorrect, Some(Misconception::MissingSolution))
        } else {
            let named = given.iter().find_map(|value| {
                self.wrong_values.iter().find(|(wrong, _)| wrong == value).map(|(_, name)| *name)
            });
            (Verdict::Incorrect, named)
        };
        Some(Reading { read: Some(read), verdict, misconception })
    }

    fn pair(&self, tokens: &[Token], unknowns: [char; 2], expected: [Rational; 2]) -> Option<Reading> {
        let found = values(tokens);
        let (first, second) = (found.of(unknowns[0]), found.of(unknowns[1]));
        let given: Vec<Option<Rational>> = match (first.last(), second.last()) {
            (None, None) => {
                let last = found.last(2);
                match last.as_slice() {
                    [a, b] => vec![Some(*a), Some(*b)],
                    [a] => vec![Some(*a), None],
                    _ => return None,
                }
            }
            (a, b) => vec![a.copied(), b.copied()],
        };
        let read = unknowns
            .iter()
            .zip(&given)
            .filter_map(|(name, value)| value.map(|value| format!("{name} = {}", show(value))))
            .collect::<Vec<_>>()
            .join(", ");
        let (verdict, misconception) = match (given[0], given[1]) {
            (Some(a), Some(b)) if a == expected[0] && b == expected[1] => (Verdict::Correct, None),
            (Some(a), None) | (None, Some(a)) if expected.contains(&a) => {
                (Verdict::Incorrect, Some(Misconception::MissingSolution))
            }
            _ => (Verdict::Incorrect, None),
        };
        Some(Reading { read: Some(read), verdict, misconception })
    }

    /// An expression to write: the last stretch of what was said that reads
    /// as one with an unknown in it, read strictly or, said aloud, with each
    /// "por" between factors.
    fn written(&self, tokens: &[Token], expected: &Poly) -> Option<Reading> {
        let mut found: Option<(Expr, Poly)> = None;
        let stretches = tokens.split(|token| {
            matches!(
                token,
                Token::Equals | Token::Or | Token::And | Token::Next | Token::Word | Token::Letter(_)
            )
        });
        for stretch in stretches.filter(|stretch| !stretch.is_empty()) {
            let readings = [expression(stretch), split_at_times(stretch)];
            let mut best: Option<(Expr, Poly)> = None;
            for expr in readings.into_iter().flatten() {
                let Some(poly) = expr.poly().filter(|poly| !poly.unknowns().is_empty()) else { continue };
                let right = poly == *expected;
                if best.is_none() || right {
                    best = Some((expr, poly));
                }
                if right {
                    break;
                }
            }
            if best.is_some() {
                found = best;
            }
        }
        let (expr, poly) = found?;
        let shape = self.shape();
        let (verdict, misconception) = if poly == *expected {
            let in_shape = match shape {
                Shape::Expanded => is_expanded(&expr),
                Shape::Factored => is_factored(&expr),
                Shape::Any => true,
            };
            (if in_shape { Verdict::Correct } else { Verdict::WrongForm }, None)
        } else {
            let named = self.wrong_polys.iter().find(|(wrong, _)| *wrong == poly).map(|(_, name)| *name);
            (Verdict::Incorrect, named)
        };
        Some(Reading { read: Some(expr.to_string()), verdict, misconception })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> Rational {
        Rational::new(n, d).unwrap()
    }

    fn verdict(exercise: &str, said: &str) -> Verdict {
        Problem::parse(exercise).unwrap().check(said).verdict
    }

    #[test]
    fn equations_are_solved_exactly() {
        let answer = |text: &str| Problem::parse(text).map(|problem| problem.answer());
        assert_eq!(answer("2x + 3 = 11").as_deref(), Some("x = 4"));
        assert_eq!(answer("3(x - 2) = x + 4").as_deref(), Some("x = 5"));
        assert_eq!(answer("x/2 + 1 = 4").as_deref(), Some("x = 6"));
        assert_eq!(answer("4a - 1 = 2").as_deref(), Some("a = 3/4"));
        assert_eq!(answer("x^2 - 5x + 6 = 0").as_deref(), Some("x = 2, x = 3"));
        assert_eq!(answer("x² = 9").as_deref(), Some("x = -3, x = 3"));
        assert_eq!(answer("x^2 + 6x + 9 = 0").as_deref(), Some("x = -3"));
        assert_eq!(answer("2x^2 + 5x - 3 = 0").as_deref(), Some("x = -3, x = 1/2"));
        assert_eq!(answer("x + y = 10; x - y = 2").as_deref(), Some("x = 6, y = 4"));
        assert_eq!(answer("2x + 3y = 12, x - y = 1").as_deref(), Some("x = 3, y = 2"));
        assert_eq!(answer("3x^2 - 1 para x = 2").as_deref(), Some("11"));
        assert_eq!(answer("Resuelve: 5x - 7 = 3x + 1").as_deref(), Some("x = 4"));
        // The examples of curriculum/math-secundaria-algebra.json, as the tutor would write them.
        for (text, expected) in [
            ("2a + 3b para a = 4 y b = −1", "5"),
            ("5a − 2b + a + 3b", "6a + b"),
            ("2(x + 1) − x = 7", "x = 5"),
            ("2x(x − 5)", "2x^2 - 10x"),
            ("(x + 3)²", "x^2 + 6x + 9"),
            ("(x + 5)(x − 5)", "x^2 - 25"),
            ("x² − 9", "(x + 3)(x - 3)"),
            ("x² − 4x = 0", "x = 0, x = 4"),
            ("x + 3x = 24", "x = 6"),
        ] {
            assert_eq!(answer(text).as_deref(), Some(expected), "{text}");
        }
        // What cannot be written exactly, or has no single answer, is not checked.
        for text in
            ["x^2 = 2", "x^2 + 1 = 0", "x + 1 = x + 2", "2x = 2x", "x + y = 3", "x^3 = 8", "x + 1", "hola"]
        {
            assert!(Problem::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn polynomials_are_expanded_or_factored() {
        let answer = |text: &str| Problem::parse(text).map(|problem| (problem.shape(), problem.answer()));
        assert_eq!(answer("(x + 2)(x - 3)"), Some((Shape::Expanded, "x^2 - x - 6".into())));
        assert_eq!(answer("(2x - 1)^2"), Some((Shape::Expanded, "4x^2 - 4x + 1".into())));
        assert_eq!(answer("3x + 2x - x + 4"), Some((Shape::Expanded, "4x + 4".into())));
        assert_eq!(answer("x^2 - 5x + 6"), Some((Shape::Factored, "(x - 2)(x - 3)".into())));
        assert_eq!(answer("2x^2 - 8"), Some((Shape::Factored, "2(x + 2)(x - 2)".into())));
        assert_eq!(answer("6xy + 9x"), Some((Shape::Factored, "3x(2y + 3)".into())));
        assert_eq!(answer("x^2 + 6x + 9"), Some((Shape::Factored, "(x + 3)^2".into())));
        assert_eq!(answer("2x^2 + 5x - 3"), Some((Shape::Factored, "(x + 3)(2x - 1)".into())));
        assert_eq!(answer("-x^2 + x"), Some((Shape::Factored, "-x(x - 1)".into())));
        // Nothing this can take out.
        assert!(Problem::parse("x^2 + 1").is_none());
        assert!(Problem::parse("x^2 - y^2").is_none());
    }

    #[test]
    fn a_solution_is_read_however_it_is_said() {
        for said in
            ["x = 4", "x vale cuatro", "equis es igual a 4", "4", "creo que es cuatro", "x=4.", "X = 4"]
        {
            assert_eq!(verdict("2x + 3 = 11", said), Verdict::Correct, "{said}");
        }
        assert_eq!(verdict("2x + 3 = 11", "le resté 3 y dividí entre 2, me dio 4"), Verdict::Correct);
        assert_eq!(verdict("2x + 3 = 11", "x = 7"), Verdict::Incorrect);
        assert_eq!(verdict("4a - 1 = 2", "a = 3/4"), Verdict::Correct);
        assert_eq!(verdict("4a - 1 = 2", "a vale tres cuartos"), Verdict::Correct);
        for said in
            ["x = 2 o x = -5", "2 y -5", "son menos cinco y dos", "x = 2 o -5", "x vale 2 y x vale menos 5"]
        {
            assert_eq!(verdict("x^2 + 3x - 10 = 0", said), Verdict::Correct, "{said}");
        }
        for said in ["x = 6, y = 4", "x vale seis y ye vale cuatro", "x=6 y=4", "6 y 4"] {
            assert_eq!(verdict("x + y = 10; x - y = 2", said), Verdict::Correct, "{said}");
        }
        assert_eq!(verdict("x + y = 10; x - y = 2", "4 y 6"), Verdict::Incorrect);
        assert_eq!(verdict("3x^2 - 1 para x = 2", "da 11"), Verdict::Correct);
        assert_eq!(verdict("2x + 3 = 11", "no sé"), Verdict::DontKnow);
        assert_eq!(verdict("2x + 3 = 11", "mmm"), Verdict::Unreadable);
    }

    #[test]
    fn an_expression_is_judged_by_its_value_and_then_by_how_it_is_written() {
        let expand = "(x + 2)(x - 3)";
        for said in ["x^2 - x - 6", "x² - x - 6", "x al cuadrado menos x menos seis", "es -6 - x + x^2"] {
            assert_eq!(verdict(expand, said), Verdict::Correct, "{said}");
        }
        // Equal, but not written expanded.
        assert_eq!(verdict(expand, "x^2 + 2x - 3x - 6"), Verdict::WrongForm);
        assert_eq!(verdict(expand, "(x + 2)(x - 3)"), Verdict::WrongForm);
        assert_eq!(verdict(expand, "x^2 - 6"), Verdict::Incorrect);

        let factor = "x^2 - 5x + 6";
        for said in [
            "(x - 2)(x - 3)",
            "(x-3)(x-2)",
            "x menos dos por x menos tres",
            "abre paréntesis x menos 3 cierra paréntesis por abre paréntesis x menos 2 cierra paréntesis",
        ] {
            assert_eq!(verdict(factor, said), Verdict::Correct, "{said}");
        }
        assert_eq!(verdict(factor, "x^2 - 5x + 6"), Verdict::WrongForm);
        assert_eq!(verdict(factor, "(x + 2)(x + 3)"), Verdict::Incorrect);
        // Taken out, but not all of it.
        assert_eq!(verdict("6x^2 + 18x", "2(3x^2 + 9x)"), Verdict::WrongForm);
        assert_eq!(verdict("6x^2 + 18x", "6x(x + 3)"), Verdict::Correct);
        assert_eq!(verdict("6x + 9", "3(2x + 3)"), Verdict::Correct);
    }

    #[test]
    fn a_known_slip_gets_its_name() {
        let named = |exercise: &str, said: &str| Problem::parse(exercise).unwrap().check(said).misconception;
        // 2x + 3 = 11 → 2x = 14: the 3 crossed without changing sign.
        assert_eq!(named("2x + 3 = 11", "x = 7"), Some(Misconception::TransposedSign));
        // 2x = 8 → x = 16, and x = 1/4.
        assert_eq!(named("2x = 8", "16"), Some(Misconception::MultipliedInstead));
        assert_eq!(named("2x = 8", "1/4"), Some(Misconception::DividedBackwards));
        assert_eq!(named("x^2 - 5x + 6 = 0", "x = 2"), Some(Misconception::MissingSolution));
        assert_eq!(named("(x + 3)^2", "x^2 + 9"), Some(Misconception::ForgotCrossTerms));
        assert_eq!(named("(x + 2)(x - 3)", "x^2 - 6"), Some(Misconception::ForgotCrossTerms));
    }

    #[test]
    fn the_tutor_saying_the_answer_is_noticed_and_stating_the_exercise_is_not() {
        let problem = Problem::parse("2x + 3 = 11").unwrap();
        assert!(!problem.is_given_away_by("Resuelve 2x + 3 = 11"));
        assert!(problem.is_given_away_by("Muy cerca: x vale 4."));
        let problem = Problem::parse("(x + 2)(x - 3)").unwrap();
        assert!(!problem.is_given_away_by("Multiplica (x + 2)(x - 3)"));
        assert!(problem.is_given_away_by("queda x^2 - x - 6"));
    }

    #[test]
    fn strange_input_does_not_break_anything() {
        let long = "x".repeat(2_000);
        for text in [
            "",
            "((((((((((((((x))))))))))))))",
            "x^99",
            "(x + 1)^6 * (x + 2)^6 * (x + 3)^6",
            "9999999999999999999x = 1",
            "x / 0 = 1",
            "= = =",
            long.as_str(),
        ] {
            let _ = Problem::parse(text);
            let _ = Problem::parse("2x + 3 = 11").unwrap().check(text);
            let _ = Problem::parse("(x + 2)(x - 3)").unwrap().check(text);
        }
        assert_eq!(exact_root(r(9, 4)), Some(r(3, 2)));
        assert_eq!(exact_root(r(2, 1)), None);
    }
}
