//! The arithmetic behind an exercise: what its answer is, and whether what
//! the student said is that answer.
//!
//! None of this is decided by a language model. The tutor states an exercise
//! as an expression ("3/4 + 1/8"), the answer is computed here with exact
//! fractions, and the student's words ("siete octavos", "7/8", "0,875") are
//! read and compared here. A model that judges arithmetic is sometimes wrong
//! in both directions, and a tutor that does not know the answer cannot give
//! it away.

use std::fmt;

/// An exact fraction, always reduced, with a positive denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rational {
    n: i64,
    d: i64,
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

impl Rational {
    pub fn new(n: i64, d: i64) -> Option<Self> {
        if d == 0 {
            return None;
        }
        let sign = if d < 0 { -1 } else { 1 };
        let g = gcd(n, d);
        Some(Self { n: n.checked_mul(sign)? / g, d: d.checked_mul(sign)? / g })
    }

    pub fn whole(n: i64) -> Self {
        Self { n, d: 1 }
    }

    pub fn plus(self, other: Self) -> Option<Self> {
        Self::new(
            self.n.checked_mul(other.d)?.checked_add(other.n.checked_mul(self.d)?)?,
            self.d.checked_mul(other.d)?,
        )
    }

    pub fn minus(self, other: Self) -> Option<Self> {
        self.plus(Self { n: other.n.checked_neg()?, d: other.d })
    }

    pub fn times(self, other: Self) -> Option<Self> {
        Self::new(self.n.checked_mul(other.n)?, self.d.checked_mul(other.d)?)
    }

    pub fn over(self, other: Self) -> Option<Self> {
        Self::new(self.n.checked_mul(other.d)?, self.d.checked_mul(other.n)?)
    }

    fn is_whole(self) -> bool {
        self.d == 1
    }
}

/// "7/8", "2", and for a fraction above one also its mixed form: "11/8 (1 3/8)".
impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_whole() {
            return write!(f, "{}", self.n);
        }
        write!(f, "{}/{}", self.n, self.d)?;
        if self.n > self.d {
            write!(f, " ({} {}/{})", self.n / self.d, self.n % self.d, self.d)?;
        }
        Ok(())
    }
}

/// A number as someone wrote or said it, before reducing: 6/8 and 3/4 are the
/// same value written two ways, and some exercises ask for one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Written {
    pub whole: Option<i64>,
    /// Numerator and denominator of the fraction part, if there is one.
    pub part: Option<(i64, i64)>,
    /// The value when it was given as a decimal, where the parts say nothing.
    decimal: Option<Rational>,
}

impl Written {
    fn integer(n: i64) -> Self {
        Self { whole: Some(n), part: None, decimal: None }
    }

    fn fraction(n: i64, d: i64) -> Self {
        Self { whole: None, part: Some((n, d)), decimal: None }
    }

    pub fn value(&self) -> Option<Rational> {
        if let Some(value) = self.decimal {
            return Some(value);
        }
        let whole = Rational::whole(self.whole.unwrap_or(0));
        match self.part {
            Some((n, d)) => whole.plus(Rational::new(n, d)?),
            None => Some(whole),
        }
    }
}

impl fmt::Display for Written {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.decimal, self.whole, self.part) {
            (Some(value), ..) => write!(f, "{value}"),
            (None, Some(whole), Some((n, d))) => write!(f, "{whole} {n}/{d}"),
            (None, Some(whole), None) => write!(f, "{whole}"),
            (None, None, Some((n, d))) => write!(f, "{n}/{d}"),
            (None, None, None) => Ok(()),
        }
    }
}

// Reading text -----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Int(i64),
    Decimal(Rational),
    /// A denominator said as a word: "cuartos", "thirds".
    Parts(i64),
    Slash,
    /// "y", "con", "enteros", "and": joins a whole number and a fraction.
    And,
    /// "sobre", "entre", "over": a fraction said top over bottom.
    Over,
    Op(char),
    Open,
    Close,
    /// "es", "da", "igual", "=": what follows is the answer.
    Marker,
    Word,
}

fn plain(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            '⁄' | '∕' => '/',
            '×' | '·' | '*' => '*',
            '÷' => ':',
            '−' | '–' => '-',
            other => other,
        })
        .collect()
}

fn cardinal(word: &str) -> Option<i64> {
    const ES: [&str; 31] = [
        "cero",
        "uno",
        "dos",
        "tres",
        "cuatro",
        "cinco",
        "seis",
        "siete",
        "ocho",
        "nueve",
        "diez",
        "once",
        "doce",
        "trece",
        "catorce",
        "quince",
        "dieciseis",
        "diecisiete",
        "dieciocho",
        "diecinueve",
        "veinte",
        "veintiuno",
        "veintidos",
        "veintitres",
        "veinticuatro",
        "veinticinco",
        "veintiseis",
        "veintisiete",
        "veintiocho",
        "veintinueve",
        "treinta",
    ];
    const EN: [&str; 21] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    let tens = |word: &str| {
        [
            ("cuarenta", 40),
            ("cincuenta", 50),
            ("sesenta", 60),
            ("setenta", 70),
            ("ochenta", 80),
            ("noventa", 90),
            ("cien", 100),
            ("thirty", 30),
            ("forty", 40),
            ("fifty", 50),
            ("sixty", 60),
            ("seventy", 70),
            ("eighty", 80),
            ("ninety", 90),
            ("hundred", 100),
        ]
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, value)| *value)
    };
    match word {
        "un" | "una" | "a" | "an" => Some(1),
        _ => ES
            .iter()
            .position(|name| *name == word)
            .or_else(|| EN.iter().position(|name| *name == word))
            .map(|value| value as i64)
            .or_else(|| tens(word)),
    }
}

/// "cuartos" is 4, "tercio" is 3, "doceavos" is 12, "halves" is 2.
fn parts(word: &str) -> Option<i64> {
    let known = [
        ("medio", 2),
        ("media", 2),
        ("mitad", 2),
        ("mitades", 2),
        ("tercio", 3),
        ("tercera", 3),
        ("cuarto", 4),
        ("cuarta", 4),
        ("quinto", 5),
        ("quinta", 5),
        ("sexto", 6),
        ("sexta", 6),
        ("septimo", 7),
        ("septima", 7),
        ("octavo", 8),
        ("octava", 8),
        ("noveno", 9),
        ("novena", 9),
        ("decimo", 10),
        ("decima", 10),
        ("centesimo", 100),
        ("half", 2),
        ("halve", 2),
        ("third", 3),
        ("quarter", 4),
        ("fourth", 4),
        ("fifth", 5),
        ("sixth", 6),
        ("seventh", 7),
        ("eighth", 8),
        ("ninth", 9),
        ("tenth", 10),
    ];
    let singular = word.strip_suffix('s').unwrap_or(word);
    if let Some((_, value)) = known.iter().find(|(name, _)| *name == singular || *name == word) {
        return Some(*value);
    }
    // "onceavos", "doceavos", "veinteavos": a cardinal with "avo".
    let stem = singular.strip_suffix("avo")?;
    cardinal(stem).or_else(|| cardinal(&format!("{stem}e"))).filter(|value| *value > 10)
}

fn decimal(text: &str) -> Option<Rational> {
    let (whole, fraction) = text.split_once(['.', ','])?;
    if whole.is_empty() || fraction.is_empty() || fraction.len() > 6 {
        return None;
    }
    let scale = 10_i64.checked_pow(fraction.len() as u32)?;
    let whole: i64 = whole.parse().ok()?;
    let fraction: i64 = fraction.parse().ok()?;
    Rational::new(whole.checked_mul(scale)?.checked_add(fraction)?, scale)
}

fn tokens(text: &str) -> Vec<Token> {
    let text = plain(text);
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            // A decimal point or comma only counts between digits.
            let decimal_follows =
                i + 1 < chars.len() && (chars[i] == '.' || chars[i] == ',') && chars[i + 1].is_ascii_digit();
            if decimal_follows {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                out.push(decimal(&word).map_or(Token::Word, Token::Decimal));
            } else {
                let word: String = chars[start..i].iter().collect();
                out.push(word.parse().map_or(Token::Word, Token::Int));
            }
            continue;
        }
        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            out.push(match word.as_str() {
                "y" | "con" | "and" | "entero" | "enteros" | "whole" | "wholes" => Token::And,
                "sobre" | "entre" | "over" => Token::Over,
                "mas" | "plus" => Token::Op('+'),
                "menos" | "minus" => Token::Op('-'),
                "por" | "times" | "de" | "of" | "x" => Token::Op('*'),
                "dividido" | "divided" => Token::Op(':'),
                "es" | "da" | "sale" | "igual" | "resultado" | "queda" | "son" | "is" | "equals"
                | "makes" => Token::Marker,
                // A unit after a number changes nothing of the arithmetic.
                "mm" | "cm" | "m" | "km" | "milimetros" | "milimetro" | "centimetros" | "centimetro"
                | "metros" | "metro" | "kilometros" | "kilometro" | "unidades" | "unidad" | "millimetres"
                | "centimetres" | "metres" | "kilometres" | "millimeters" | "centimeters" | "meters"
                | "kilometers" | "units" | "unit" => continue,
                _ => match (parts(&word), cardinal(&word)) {
                    // "un cuarto" is a quarter, but "cuarto" alone after a number is too.
                    (Some(value), _) => Token::Parts(value),
                    (None, Some(value)) => Token::Int(value),
                    (None, None) => Token::Word,
                },
            });
            continue;
        }
        out.push(match c {
            '/' => Token::Slash,
            '+' | '-' | '*' | ':' => Token::Op(c),
            '(' => Token::Open,
            ')' => Token::Close,
            '=' => Token::Marker,
            _ => {
                i += 1;
                continue;
            }
        });
        i += 1;
    }
    out
}

/// The number that starts at `tokens[0]`, and how many tokens it took.
/// Longer forms first: "2 y 1/3" before "2".
fn number(tokens: &[Token]) -> Option<(Written, usize)> {
    use Token::{And, Decimal, Int, Over, Parts, Slash};
    let fraction = |n: i64, d: i64| (d != 0).then(|| Written::fraction(n, d));
    let mixed = |whole: i64, n: i64, d: i64| {
        (d != 0).then_some(Written { whole: Some(whole), part: Some((n, d)), decimal: None })
    };
    Some(match tokens {
        [Int(w), And, And, Int(n), Slash, Int(d), ..] => (mixed(*w, *n, *d)?, 6),
        [Int(w), And, And, Int(n), Parts(d), ..] => (mixed(*w, *n, *d)?, 5),
        [Int(w), And, Int(n), Slash, Int(d), ..] => (mixed(*w, *n, *d)?, 5),
        [Int(w), And, Int(n), Parts(d), ..] => (mixed(*w, *n, *d)?, 4),
        [Int(w), Int(n), Slash, Int(d), ..] => (mixed(*w, *n, *d)?, 4),
        // "uno y medio"
        [Int(w), And, Parts(d), ..] => (mixed(*w, 1, *d)?, 3),
        [Int(n), Slash, Int(d), ..] | [Int(n), Over, Int(d), ..] => (fraction(*n, *d)?, 3),
        [Int(n), Parts(d), ..] => (fraction(*n, *d)?, 2),
        [Decimal(value), ..] => (Written { whole: None, part: None, decimal: Some(*value) }, 1),
        [Int(n), ..] => (Written::integer(*n), 1),
        // "la mitad", "medio"
        [Parts(d), ..] => (fraction(1, *d)?, 1),
        _ => return None,
    })
}

/// The number someone gave as their answer, out of whatever else they said.
/// What follows the last "es", "da" or "=" wins; otherwise the first number.
pub fn read_answer(text: &str) -> Option<Written> {
    let tokens = tokens(text);
    // "7/0" is not a number, and neither is the 0 left over from it.
    if tokens.windows(2).any(|pair| matches!(pair, [Token::Slash | Token::Over, Token::Int(0)])) {
        return None;
    }
    let after_marker = tokens.iter().rposition(|token| *token == Token::Marker).map_or(0, |at| at + 1);
    let first =
        |from: usize| (from..tokens.len()).find_map(|at| number(&tokens[at..]).map(|(written, _)| written));
    first(after_marker).or_else(|| first(0))
}

/// Every number written or said in a text, in order.
pub fn numbers(text: &str) -> Vec<Rational> {
    let tokens = tokens(text);
    let mut found = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        match number(&tokens[at..]) {
            Some((written, used)) => {
                found.extend(written.value());
                at += used.max(1);
            }
            None => at += 1,
        }
    }
    found
}

// Exercises -------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

/// An exercise the tutor stated, with its answer worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct Exercise {
    pub answer: Rational,
    /// The two numbers and the operation, when the exercise is exactly that:
    /// it is what lets a wrong answer be recognised as a known mistake.
    simple: Option<(Written, Operation, Written)>,
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn sum(&mut self) -> Option<Rational> {
        let mut value = self.product()?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.product()?;
            value = if op == '+' { value.plus(right)? } else { value.minus(right)? };
        }
        Some(value)
    }

    fn product(&mut self) -> Option<Rational> {
        let mut value = self.operand()?;
        loop {
            match self.peek() {
                Some(Token::Op('*')) => {
                    self.at += 1;
                    value = value.times(self.operand()?)?;
                }
                Some(Token::Op(':')) => {
                    self.at += 1;
                    // "dividido entre": the word after "dividido" is not a fraction bar here.
                    if self.peek() == Some(&Token::Over) {
                        self.at += 1;
                    }
                    value = value.over(self.operand()?)?;
                }
                _ => return Some(value),
            }
        }
    }

    fn operand(&mut self) -> Option<Rational> {
        if self.peek() == Some(&Token::Open) {
            self.at += 1;
            let value = self.sum()?;
            (self.peek() == Some(&Token::Close)).then(|| self.at += 1)?;
            return Some(value);
        }
        let (written, used) = number(&self.tokens[self.at..])?;
        self.at += used;
        written.value()
    }
}

impl Exercise {
    /// Whether a text of the tutor's says this exercise's answer. A number that
    /// is already in the exercise as asked ("5 x 1") does not count: saying
    /// the exercise is not giving it away.
    pub fn is_given_away_by(&self, expression: &str, said: &str) -> bool {
        !numbers(expression).contains(&self.answer) && numbers(said).contains(&self.answer)
    }

    /// Reads an expression such as "3/4 + 1/8", "2 1/2 x 4", "3/4 de 20" or
    /// "(1/2 + 1/3) : 2". None when it is not arithmetic this can work out.
    pub fn parse(expression: &str) -> Option<Self> {
        let mut tokens = tokens(expression);
        // "6 x 4 = ?" is still 6 x 4: what follows the last number is not part of it.
        while tokens.last() == Some(&Token::Marker) {
            tokens.pop();
        }
        if tokens.is_empty() || tokens.iter().any(|token| matches!(token, Token::Word | Token::Marker)) {
            return None;
        }
        let mut parser = Parser { tokens: &tokens, at: 0 };
        let answer = parser.sum()?;
        if parser.at != tokens.len() {
            return None;
        }
        Some(Self { answer, simple: simple(&tokens) })
    }
}

fn simple(tokens: &[Token]) -> Option<(Written, Operation, Written)> {
    let (left, used) = number(tokens)?;
    let operation = match tokens.get(used)? {
        Token::Op('+') => Operation::Add,
        Token::Op('-') => Operation::Subtract,
        Token::Op('*') => Operation::Multiply,
        Token::Op(':') => Operation::Divide,
        _ => return None,
    };
    let rest = &tokens[used + 1..];
    let rest =
        if operation == Operation::Divide && rest.first() == Some(&Token::Over) { &rest[1..] } else { rest };
    let (right, used) = number(rest)?;
    (used == rest.len()).then_some((left, operation, right))
}

/// How the answer must be written to count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// Any way of writing the right value: 2/4 for 1/2, 0.5, 1 1/2 or 3/2.
    #[default]
    Any,
    /// A fraction in lowest terms, or a whole number.
    Simplified,
    /// A whole number and a proper fraction when the value is above one.
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Correct,
    /// The right value, not written the way the exercise asks.
    WrongForm,
    Incorrect,
    /// No number could be made out: ask again, it does not count.
    Unreadable,
    /// "No sé": not a wrong answer, a request for help.
    DontKnow,
    /// Numbers, but about something else: another calculation or a question
    /// of the student's own. It does not count as a try.
    NotAnAnswer,
}

/// A mistake with a name: it tells the tutor what the student did, so the
/// next question can aim at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Misconception {
    /// 1/2 + 1/3 = 2/5: tops with tops and bottoms with bottoms.
    AddedAcross,
    /// 1/2 + 1/3 = 2/3 or 2/2: added the tops and kept one of the bottoms.
    KeptOneDenominator,
    /// Gave the answer upside down: 8/3 for 3/8.
    Inverted,
    /// Multiplied where the exercise divides.
    MultipliedInstead,
    /// Divided where the exercise multiplies.
    DividedInstead,
    /// Added where the exercise subtracts, or the other way round.
    WrongSign,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The number that was understood, as it was given.
    pub read: Option<String>,
    pub verdict: Verdict,
    pub misconception: Option<Misconception>,
}

fn in_form(written: &Written, value: Rational, form: Form) -> bool {
    let reduced = |(n, d): (i64, i64)| gcd(n, d) == 1 && d > 1;
    match form {
        Form::Any => true,
        _ if written.decimal.is_some() => false,
        _ if value.is_whole() => written.part.is_none(),
        Form::Simplified => {
            written.part.is_some_and(reduced)
                && written.whole.is_none_or(|_| written.part.is_some_and(|(n, d)| n < d))
        }
        Form::Mixed => match (written.whole, written.part) {
            (Some(_), Some((n, d))) => n < d && reduced((n, d)),
            (None, Some((n, d))) => n < d && reduced((n, d)),
            _ => false,
        },
    }
}

fn misconception(exercise: &Exercise, given: Rational) -> Option<Misconception> {
    use Misconception::{
        AddedAcross, DividedInstead, Inverted, KeptOneDenominator, MultipliedInstead, WrongSign,
    };
    use Operation::{Add, Divide, Multiply, Subtract};
    let answer = exercise.answer;
    let is = |candidate: Option<Rational>| candidate.is_some_and(|value| value == given && value != answer);
    if let Some((left, operation, right)) = &exercise.simple {
        let (a, b) = (left.value()?, right.value()?);
        // The parts as written: 2/4 + 1/4 added across is 3/8, not from 1/2.
        let across = |sign: i64| match (left.part, right.part, left.whole, right.whole) {
            (Some((an, ad)), Some((bn, bd)), None, None) => Some((an + sign * bn, ad + sign * bd, ad, bd)),
            _ => None,
        };
        match operation {
            Add | Subtract => {
                let sign = if *operation == Add { 1 } else { -1 };
                if let Some((n, d, ad, bd)) = across(sign) {
                    if is(Rational::new(n, d)) {
                        return Some(AddedAcross);
                    }
                    if ad != bd && (is(Rational::new(n, ad)) || is(Rational::new(n, bd))) {
                        return Some(KeptOneDenominator);
                    }
                }
                if is(if *operation == Add { a.minus(b) } else { a.plus(b) }) {
                    return Some(WrongSign);
                }
            }
            Multiply if is(a.over(b)) => return Some(DividedInstead),
            Divide if is(a.times(b)) => return Some(MultipliedInstead),
            _ => {}
        }
    }
    is(Rational::whole(1).over(answer)).then_some(Inverted)
}

/// "No sé", "ni idea", "I don't know": said instead of an answer.
pub fn says_dont_know(said: &str) -> bool {
    let said = plain(said);
    let words: Vec<&str> =
        said.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect();
    let has = |phrase: &[&str]| words.windows(phrase.len()).any(|window| window == phrase);
    has(&["no", "se"])
        || has(&["no", "lo", "se"])
        || has(&["ni", "idea"])
        || has(&["no", "entiendo"])
        || has(&["no", "puedo"])
        || has(&["don", "t", "know"])
        || has(&["dont", "know"])
        || has(&["no", "idea"])
}

/// Whether what was said is about something other than this exercise: it
/// states a calculation that shares no number with it ("siete por ocho es
/// 54" while "3 x 4" is open), or asks a question with numbers of its own
/// ("una cancha mide 18 de largo y 9 de ancho, ¿cuánto mide su perímetro?").
/// Deliberately narrow: an explanation that names the exercise's own numbers,
/// or a bare number, is always an answer.
pub fn is_about_something_else(expression: &str, said: &str) -> bool {
    // The exercise's numbers, and the tops and bottoms of its fractions: an
    // explanation works with those too.
    let mut own = numbers(expression);
    own.extend(tokens(expression).iter().filter_map(|token| match token {
        Token::Int(value) | Token::Parts(value) => Some(Rational::whole(*value)),
        _ => None,
    }));
    let tokens = tokens(said);
    let mut stated = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let Some((left, used)) = number(&tokens[at..]) else {
            at += 1;
            continue;
        };
        let mut next = at + used;
        if matches!(tokens.get(next), Some(Token::Op(_))) {
            next += 1;
            if tokens.get(next) == Some(&Token::Over) {
                next += 1;
            }
            if let Some((right, _)) = tokens.get(next..).and_then(number) {
                stated.push((left.value(), right.value()));
            }
        }
        at += used.max(1);
    }
    let shares = |value: &Option<Rational>| value.is_some_and(|value| own.contains(&value));
    if !stated.is_empty() {
        return stated.iter().all(|(left, right)| !shares(left) && !shares(right));
    }
    let text = plain(said);
    let asks = text.contains('?')
        && text.split(|c: char| !c.is_alphanumeric()).any(|word| {
            matches!(word, "cuanto" | "cuantos" | "cuanta" | "cuantas" | "cual" | "how" | "what" | "which")
        });
    let said = numbers(said);
    asks && said.len() >= 2 && said.iter().all(|value| !own.contains(value))
}

/// Judges what the student said against the exercise.
pub fn check(exercise: &Exercise, form: Form, said: &str) -> Reading {
    let Some(written) = read_answer(said) else {
        let verdict = if says_dont_know(said) { Verdict::DontKnow } else { Verdict::Unreadable };
        return Reading { read: None, verdict, misconception: None };
    };
    let read = Some(written.to_string());
    let Some(given) = written.value() else {
        return Reading { read, verdict: Verdict::Unreadable, misconception: None };
    };
    if given != exercise.answer {
        return Reading { read, verdict: Verdict::Incorrect, misconception: misconception(exercise, given) };
    }
    let verdict = if in_form(&written, given, form) { Verdict::Correct } else { Verdict::WrongForm };
    Reading { read, verdict, misconception: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> Rational {
        Rational::new(n, d).unwrap()
    }

    fn answer(expression: &str) -> Rational {
        Exercise::parse(expression).unwrap_or_else(|| panic!("{expression} was not read")).answer
    }

    fn value(said: &str) -> Option<Rational> {
        read_answer(said).and_then(|written| written.value())
    }

    #[test]
    fn expressions_are_worked_out_exactly() {
        for (expression, expected) in [
            ("3/4 + 1/8", r(7, 8)),
            ("1/2 + 1/3", r(5, 6)),
            ("3/4 - 1/4", r(1, 2)),
            ("2/3 × 3/4", r(1, 2)),
            ("2/3 x 3/4", r(1, 2)),
            ("3/4 : 1/8", r(6, 1)),
            ("3/4 ÷ 1/8", r(6, 1)),
            ("3/4 de 20", r(15, 1)),
            ("2 1/2 + 1/2", r(3, 1)),
            ("(1/2 + 1/3) : 2", r(5, 12)),
            ("1/2 + 1/3 * 3", r(3, 2)),
            ("12 + 7", r(19, 1)),
            ("0,5 + 0.25", r(3, 4)),
            ("tres cuartos más un octavo", r(7, 8)),
            ("un medio por cuatro", r(2, 1)),
            ("seis dividido entre tres cuartos", r(8, 1)),
        ] {
            assert_eq!(answer(expression), expected, "{expression}");
        }
    }

    #[test]
    fn a_tutor_who_says_the_answer_is_noticed_and_one_who_states_the_exercise_is_not() {
        let expression = "3/4 + 1/8";
        let exercise = Exercise::parse(expression).unwrap();
        assert!(exercise.is_given_away_by(expression, "Muy cerca: es siete octavos, ¿ves por qué?"));
        assert!(exercise.is_given_away_by(expression, "La respuesta es 7/8."));
        assert!(!exercise.is_given_away_by(expression, "¿Cuánto es tres cuartos más un octavo?"));
        assert!(!exercise.is_given_away_by(expression, "Piensa en cuántos octavos hay en 3/4."));
        // The answer is one of the numbers asked: stating the exercise is not a leak.
        let same = Exercise::parse("5 x 1").unwrap();
        assert!(!same.is_given_away_by("5 x 1", "¿Cuánto es 5 por 1?"));
    }

    #[test]
    fn units_and_a_trailing_question_do_not_stop_an_exercise_from_being_checked() {
        let whole = |n: i64| Exercise::parse(&n.to_string()).unwrap().answer;
        // A perimeter, as a tutor writes it.
        assert_eq!(Exercise::parse("3 cm + 4 cm + 5 cm").unwrap().answer, whole(12));
        assert_eq!(Exercise::parse("6 × 4 cm = ?").unwrap().answer, whole(24));
        assert_eq!(Exercise::parse("6 por 4 centímetros").unwrap().answer, whole(24));
        assert_eq!(Exercise::parse("8 m - 3 m =").unwrap().answer, whole(5));
        // Words that are not a unit still mean it is not plain arithmetic.
        assert!(Exercise::parse("el perímetro del hexágono").is_none());
        assert!(Exercise::parse("3 manzanas + 4 peras").is_none());
        assert!(Exercise::parse("= ?").is_none());
        // A student's answer with its unit reads as the number.
        assert_eq!(read_answer("son 24 centímetros").and_then(|written| written.value()), Some(whole(24)));
    }

    #[test]
    fn what_is_not_arithmetic_is_not_an_exercise() {
        for expression in
            ["", "cuántas pelotas rojas hay", "3/0 + 1", "3/4 +", "(1/2 + 1/3", "3/4 + x", "1/2 = 2/4"]
        {
            assert!(Exercise::parse(expression).is_none(), "{expression}");
        }
        // Numbers too large to work with are refused, not wrapped around.
        assert!(Exercise::parse("9223372036854775807 * 9223372036854775807").is_none());
    }

    #[test]
    fn an_answer_is_read_however_it_is_said() {
        for (said, expected) in [
            ("7/8", r(7, 8)),
            ("7 / 8", r(7, 8)),
            ("siete octavos", r(7, 8)),
            ("Siete octavos, creo.", r(7, 8)),
            ("me sale siete sobre ocho", r(7, 8)),
            ("un medio", r(1, 2)),
            ("la mitad", r(1, 2)),
            ("tres cuartos", r(3, 4)),
            ("dos tercios", r(2, 3)),
            ("cinco doceavos", r(5, 12)),
            ("1 1/2", r(3, 2)),
            ("uno y medio", r(3, 2)),
            ("dos enteros y un cuarto", r(9, 4)),
            ("2 y 1/3", r(7, 3)),
            ("0,75", r(3, 4)),
            ("0.875", r(7, 8)),
            ("quince", r(15, 1)),
            ("son 15 pelotas", r(15, 1)),
            ("three quarters", r(3, 4)),
            ("one and a half", r(3, 2)),
            ("seven over eight", r(7, 8)),
            // What follows "es" or "da" is the answer, not the numbers repeated before it.
            ("tres cuartos más un octavo es siete octavos", r(7, 8)),
            ("3/4 + 1/8 = 7/8", r(7, 8)),
            ("primero pensé cinco sextos pero me da siete octavos", r(7, 8)),
        ] {
            assert_eq!(value(said), Some(expected), "{said}");
        }
        for said in ["", "no sé", "mmm", "no entiendo la pregunta", "7/0"] {
            assert_eq!(value(said), None, "{said}");
        }
    }

    #[test]
    fn the_value_decides_and_then_the_form() {
        let exercise = Exercise::parse("1/4 + 1/4").unwrap();
        let verdict = |form, said| check(&exercise, form, said).verdict;
        for said in ["1/2", "2/4", "un medio", "dos cuartos", "0,5"] {
            assert_eq!(verdict(Form::Any, said), Verdict::Correct, "{said}");
        }
        assert_eq!(verdict(Form::Simplified, "un medio"), Verdict::Correct);
        assert_eq!(verdict(Form::Simplified, "2/4"), Verdict::WrongForm);
        assert_eq!(verdict(Form::Simplified, "0.5"), Verdict::WrongForm);
        assert_eq!(verdict(Form::Any, "1/3"), Verdict::Incorrect);
        assert_eq!(verdict(Form::Any, "no sé"), Verdict::DontKnow);
        assert_eq!(verdict(Form::Any, "mmm, ni idea"), Verdict::DontKnow);
        assert_eq!(verdict(Form::Any, "I don't know"), Verdict::DontKnow);
        assert_eq!(verdict(Form::Any, "¿cómo?"), Verdict::Unreadable);
        // A number wins over the doubt around it.
        assert_eq!(verdict(Form::Any, "no sé, ¿un medio?"), Verdict::Correct);

        let above_one = Exercise::parse("3/4 + 3/4").unwrap();
        let verdict = |form, said| check(&above_one, form, said).verdict;
        assert_eq!(verdict(Form::Mixed, "1 1/2"), Verdict::Correct);
        assert_eq!(verdict(Form::Mixed, "uno y medio"), Verdict::Correct);
        assert_eq!(verdict(Form::Mixed, "3/2"), Verdict::WrongForm);
        assert_eq!(verdict(Form::Mixed, "1 2/4"), Verdict::WrongForm);
        assert_eq!(verdict(Form::Simplified, "3/2"), Verdict::Correct);
        assert_eq!(verdict(Form::Simplified, "6/4"), Verdict::WrongForm);

        let whole = Exercise::parse("3/4 de 20").unwrap();
        assert_eq!(check(&whole, Form::Simplified, "quince").verdict, Verdict::Correct);
        assert_eq!(check(&whole, Form::Mixed, "15").verdict, Verdict::Correct);
        assert_eq!(check(&whole, Form::Simplified, "30/2").verdict, Verdict::WrongForm);
    }

    #[test]
    fn a_known_mistake_gets_its_name() {
        let named = |expression: &str, said: &str| {
            let reading = check(&Exercise::parse(expression).unwrap(), Form::Any, said);
            assert_eq!(reading.verdict, Verdict::Incorrect, "{expression} -> {said}");
            reading.misconception
        };
        assert_eq!(named("1/2 + 1/3", "2/5"), Some(Misconception::AddedAcross));
        assert_eq!(named("1/2 + 1/3", "dos quintos"), Some(Misconception::AddedAcross));
        assert_eq!(named("2/4 + 1/4", "3/8"), Some(Misconception::AddedAcross));
        assert_eq!(named("3/4 - 1/2", "2/2"), Some(Misconception::AddedAcross));
        assert_eq!(named("1/2 + 1/3", "2/3"), Some(Misconception::KeptOneDenominator));
        assert_eq!(named("3/4 - 1/8", "2/8"), Some(Misconception::KeptOneDenominator));
        assert_eq!(named("3/4 + 1/8", "5/8"), Some(Misconception::WrongSign));
        assert_eq!(named("2/3 x 3/4", "8/9"), Some(Misconception::DividedInstead));
        assert_eq!(named("3/4 : 1/8", "3/32"), Some(Misconception::MultipliedInstead));
        assert_eq!(named("3/4 : 2", "8/3"), Some(Misconception::Inverted));
        // A wrong answer nobody has a name for stays just wrong.
        assert_eq!(named("1/2 + 1/3", "7/9"), None);
        // The right answer is never called a mistake, whatever it coincides with.
        let same = Exercise::parse("1/2 + 1/2").unwrap();
        assert_eq!(
            check(&same, Form::Any, "1"),
            Reading { read: Some("1".into()), verdict: Verdict::Correct, misconception: None }
        );
    }

    #[test]
    fn an_answer_is_shown_plainly() {
        assert_eq!(r(7, 8).to_string(), "7/8");
        assert_eq!(r(6, 3).to_string(), "2");
        assert_eq!(r(11, 8).to_string(), "11/8 (1 3/8)");
        assert_eq!(read_answer("dos enteros y un cuarto").unwrap().to_string(), "2 1/4");
        assert_eq!(read_answer("0,75").unwrap().to_string(), "3/4");
    }
    #[test]
    fn talk_about_another_calculation_is_not_an_answer() {
        // Another calculation, or a question with numbers of its own.
        assert!(is_about_something_else("3 x 4", "Seven times eight is 54"));
        assert!(is_about_something_else("3 x 4", "siete por ocho es 54"));
        assert!(is_about_something_else(
            "6 + 8 + 10",
            "El siguiente: una cancha mide 18 metros de largo y 9 de ancho. ¿Cuánto mide su perímetro?"
        ));
        // An answer, however it is said, stays an answer.
        for said in ["24", "¿54 metros?", "Creo que es 20 centímetros", "Oh, it's 56", "no sé"] {
            assert!(!is_about_something_else("6 + 8 + 10", said), "{said}");
        }
        assert!(!is_about_something_else("5 + 7 + 9", "sumé 5 más 7 es 12, más 9 es 21"));
        assert!(!is_about_something_else("2/3 + 1/4", "8 más 3 es 11, entonces 11/12"));
        assert!(!is_about_something_else("3 x 4", "tres por cuatro es 11"));
    }
}
