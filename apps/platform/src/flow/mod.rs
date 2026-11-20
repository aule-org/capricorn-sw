//! A session run by the session engine (`engine/`).
//!
//! The engine decides what happens: which stage the student is in, the next
//! exercise, when a hint is due, when the session ends. This module drives it:
//! it turns what the student said into the engine's inputs, and what the
//! engine answers into lines for the tutor to say and things for the screen
//! to show. The tutor's model words a reply only where the engine leaves the
//! conversation free, and then under the engine's rules for that moment.
//!
//! The engine is not serializable and reads the time only from the clock it
//! is given. A session is therefore kept as the ordered inputs that went into
//! it, each with the clock at which it was applied; applying them again gives
//! the same engine. Live engines are cached in the process and rebuilt from
//! the table when they are missing or may have fallen behind it.
//!
//! The inputs hold what the student said. Nothing from them is logged, and
//! they are deleted with the session's turns (see `sessions::review_session`,
//! `jobs::housekeeping` and `families::record_consent`).

mod visuals;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use aule_core::banco::Banco;
use aule_core::conversacion::{self, Intencion};
use aule_core::correccion::{self, Clave, Veredicto};
use aule_core::modelo::{EstadoObjetivo, Objetivo, Perfil, TipoObjetivo};
use aule_core::motor::{Adjunto, Calificacion, Entrada, Esperando, Mensaje, Modo, Motor, Redaccion, Tarjeta};
use aule_core::reloj::Reloj;
use aule_core::texto;
use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{Agent, Caller, Input as Body, Session};
use crate::error::{ApiError, ApiResult};
use crate::families::{child_for, find_child};
use crate::state::AppState;

/// The value of `sessions.flow` for a session the engine runs.
pub const ENGINE: &str = "engine";
/// The subject, as families name it, of the bank compiled into the binary.
const BANK_SUBJECT: &str = "matematica";
/// The engine's texts exist in Spanish only.
const BANK_LANGUAGE: &str = "es";
const MAX_SAID_CHARS: usize = 500;
/// The engine works with exact fractions on 64-bit integers. A number longer
/// than this is not an answer to any exercise, and is kept away from that
/// arithmetic so it cannot overflow.
const MAX_DIGITS: usize = 6;
/// Said once when an answer came without how it was worked out, in the stages
/// that ask for both. It says nothing about whether the answer is right.
const ASK_HOW: &str = "¡Gracias! Ahora cuéntame cómo lo pensaste.";
/// The report greets the guardian by name; a family always has one.
const GUARDIAN_FALLBACK: &str = "familia";

/// The bank compiled into the binary, checked once. Nothing when it fails
/// its own validation: then no session is run by the engine.
fn bank() -> Option<Arc<Banco>> {
    static BANK: OnceLock<Option<Arc<Banco>>> = OnceLock::new();
    BANK.get_or_init(|| match contenido::cargar_validado() {
        Ok(bank) => Some(Arc::new(bank)),
        Err(problems) => {
            tracing::error!(problems = problems.len(), "the engine's bank is not valid; the engine is off");
            None
        }
    })
    .clone()
}

/// The engine's clock: milliseconds since the session's first input, set by
/// the platform before every input and never read from the system.
#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);

impl Reloj for Clock {
    fn ahora_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// What a session is for, in the engine's terms.
///
/// | The family's objective (`kind`) | The engine's `TipoObjetivo` |
/// |---|---|
/// | `homework` | `Tarea`, with the bank's own homework as the exercises |
/// | `exam_prep` | `Preparacion`: the engine simulates the exam |
/// | `explore` | no objective: `Extracurricular`, with the topic as the family's suggestion |
/// | `exam_review` | not run by the engine |
///
/// `exam_review` is left out because the engine's `Correccion` works on an
/// exam the student already sat: which questions they missed and what they
/// answered. The platform does not have that, and the bank's sample exam
/// would be presented to the student as their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Homework,
    ExamPrep,
    Explore,
}

/// The objective as the engine can run it, or nothing when the bank does not
/// cover it: today the bank is fractions in mathematics, in Spanish.
pub fn goal(objective: Option<&Value>, language: &str) -> Option<(Goal, Option<NaiveDate>)> {
    let (objective, bank) = (objective?, bank()?);
    if language != BANK_LANGUAGE {
        return None;
    }
    // A subject left out is decided by the topic; any other subject is not ours.
    if objective["subject"].as_str().is_some_and(|subject| subject != BANK_SUBJECT) {
        return None;
    }
    // "Fracciones equivalentes" and "suma de fracciones" are both the bank's topic.
    let stem: String = texto::normalizar(&bank.tema).chars().take(7).collect();
    if !texto::normalizar(objective["topic"].as_str()?).contains(&stem) {
        return None;
    }
    let goal = match objective["kind"].as_str()? {
        "homework" => Goal::Homework,
        "exam_prep" => Goal::ExamPrep,
        "explore" => Goal::Explore,
        _ => return None,
    };
    let due = objective["due"].as_str().and_then(|due| due.parse().ok());
    Some((goal, due))
}

/// What an engine is started with. A session rebuilt later is started with
/// the same, read again from the database.
///
/// The child becomes the engine's `Perfil`: the nickname is the student's
/// name, the interests (joined) choose the setting of the exercises, the
/// family's first guardian is who the report greets. The engine's level per
/// topic is not kept between sessions yet, so every session starts unknown.
#[derive(Clone, Debug)]
pub struct Setup {
    pub student: String,
    pub interests: Vec<String>,
    pub guardian: String,
    /// The day of the session where the family lives.
    pub today: NaiveDate,
    pub goal: Goal,
    pub due: Option<NaiveDate>,
}

fn engine(setup: &Setup, bank: Arc<Banco>, clock: Clock) -> Motor {
    let mut profile =
        Perfil::nuevo(&setup.student, &setup.interests.join(", "), &setup.guardian, setup.today);
    let objective = |kind: TipoObjetivo| Objetivo {
        id: 1,
        tipo: kind,
        // The engine matches an objective to the bank by these two names.
        curso: bank.curso.clone(),
        tema: bank.tema.clone(),
        fecha: setup.due,
        sin_fecha_confirmado: false,
        nivel_objetivo: None,
        tarea_id: (kind == TipoObjetivo::Tarea).then(|| bank.tareas.first().map(|t| t.id.clone())).flatten(),
        evaluacion_id: None,
        falladas: Vec::new(),
        estado: EstadoObjetivo::Pendiente,
    };
    let objectives = match setup.goal {
        Goal::Homework => vec![objective(TipoObjetivo::Tarea)],
        Goal::ExamPrep => vec![objective(TipoObjetivo::Preparacion)],
        Goal::Explore => {
            profile.sugerencia_padre = Some(bank.tema.clone());
            Vec::new()
        }
    };
    Motor::new(aule_core::config::Config::default(), bank, Arc::new(clock), profile, objectives)
}

/// The platform's own copy of what can go into the engine, so what is stored
/// does not depend on the engine's types. `Hold` is the platform's: an answer
/// kept until the student says how they worked it out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Input {
    Start,
    Text {
        text: String,
    },
    Graded {
        read: Option<String>,
        correct: bool,
        wrong_form: bool,
        explained: Option<bool>,
        unreadable: bool,
    },
    Help,
    Tired,
    LeaveScreening,
    Activity {
        choice: usize,
    },
    Tick,
    Chat {
        text: String,
        intent: Intencion,
    },
    Hold {
        said: String,
    },
}

/// An input and the engine's clock when it was applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub at_ms: u64,
    pub input: Input,
}

/// One thing for the tutor to say, and what to put on the screen with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// The engine's text, word for word.
    pub say: String,
    pub show: Option<Value>,
    /// The engine would have a model word this one; `say` is its fallback.
    pub reword: bool,
    /// The line answers something the student said in their own words, so it
    /// is not treated as fixed text.
    pub echoes: bool,
}

impl Line {
    fn fixed(say: &str, show: Option<Value>) -> Self {
        Self { say: say.to_string(), show, reword: false, echoes: false }
    }
}

/// What the tutor does with one thing the student said.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub lines: Vec<Line>,
    /// Rules for a reply in the model's own words after the lines, when the
    /// engine leaves this turn free. Nothing when the lines are the whole turn.
    pub converse: Option<String>,
    pub closed: bool,
}

struct Held {
    card: String,
    said: String,
}

/// Whether the lines the engine answered with are the whole turn.
#[derive(PartialEq)]
enum Reply {
    Scripted,
    Free,
}

/// An engine and the inputs it has taken.
pub struct Flow {
    motor: Motor,
    clock: Clock,
    setup: Setup,
    bank: Arc<Banco>,
    /// Inputs applied and not stored yet.
    unsaved: Vec<Record>,
    /// How many inputs are stored: the place of the next one.
    stored: usize,
    last_ms: u64,
    held: Option<Held>,
}

fn has_long_number(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_digit()).any(|digits| digits.len() > MAX_DIGITS)
}

fn is_question(said: &str) -> bool {
    let words = texto::normalizar(said);
    said.contains(['?', '¿'])
        || ["que ", "como ", "cual ", "cuanto ", "por que "].iter().any(|start| words.starts_with(start))
}

/// The student asks to stop the screening, which the engine tells them they may.
fn wants_out(said: &str) -> bool {
    let said = texto::normalizar(said);
    ["salir", "abrumad", "ya no quiero", "no quiero seguir"].iter().any(|phrase| said.contains(phrase))
}

/// The part of a spoken answer that is the answer itself: what comes before
/// "porque" or the end of the first sentence, when a reading of `readable`
/// is there. The engine's reader takes the number after the last "es" or
/// "son", which in "tres octavos porque el ocho es el total" is not the answer.
fn answer_part(said: &str, readable: impl Fn(&str) -> bool) -> &str {
    let mut cut = said.find(". ");
    let mut words = said.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let offset = word.as_ptr() as usize - said.as_ptr() as usize;
        let next = words.peek().map(|next| texto::normalizar(next));
        let connector = match texto::normalizar(word).as_str() {
            "porque" | "pues" => true,
            "por" | "ya" => next.as_deref() == Some("que"),
            _ => false,
        };
        if connector {
            cut = Some(cut.map_or(offset, |cut| cut.min(offset)));
            break;
        }
    }
    match cut.map(|cut| said[..cut].trim()) {
        Some(part) if !part.is_empty() && readable(part) => part,
        _ => said,
    }
}

fn card_key(card: &Tarjeta) -> String {
    format!("{:?}|{}", card.modo, card.id.as_deref().unwrap_or(&card.enunciado))
}

/// The statement of the card, to say and to show: its drawing, or its text
/// when it has none or the screen cannot draw it.
fn card_line(card: &Tarjeta) -> Line {
    let show = card
        .grafico
        .as_ref()
        .and_then(visuals::drawing)
        .or_else(|| visuals::statement(None, &card.enunciado));
    Line::fixed(&card.enunciado, show)
}

fn lines_of(messages: Vec<Mensaje>, echoes: bool, out: &mut Vec<Line>) {
    for message in messages {
        out.push(Line {
            show: message.adjunto.as_ref().and_then(visuals::attachment),
            reword: matches!(message.redaccion, Redaccion::Ia { .. }),
            say: message.texto,
            echoes,
        });
        // A worked example lives in the attachment, which a page shows whole.
        // A voice has to say it: the statement, then each step.
        if let Some(Adjunto::Pasos { enunciado, pasos, .. }) = &message.adjunto {
            out.push(Line::fixed(enunciado, None));
            out.extend(pasos.iter().map(|step| Line::fixed(step, None)));
        }
    }
}

impl Flow {
    pub fn new(setup: Setup, bank: Arc<Banco>) -> Self {
        let clock = Clock::default();
        Self {
            motor: engine(&setup, bank.clone(), clock.clone()),
            clock,
            setup,
            bank,
            unsaved: Vec::new(),
            stored: 0,
            last_ms: 0,
            held: None,
        }
    }

    /// The engine that took `records`, in order, each at its own time.
    pub fn replay(setup: Setup, bank: Arc<Banco>, records: &[Record]) -> Self {
        let mut flow = Self::new(setup, bank);
        for record in records {
            flow.feed(record.at_ms, &record.input);
        }
        flow.stored = records.len();
        flow
    }

    pub fn started(&self) -> bool {
        self.stored + self.unsaved.len() > 0
    }

    pub fn closed(&self) -> bool {
        self.motor.terminada()
    }

    /// The inputs applied since the last `saved`.
    pub fn unsaved(&self) -> &[Record] {
        &self.unsaved
    }

    pub fn stored(&self) -> usize {
        self.stored
    }

    pub fn saved(&mut self) {
        self.stored += self.unsaved.len();
        self.unsaved.clear();
    }

    // The clock never goes back, whatever the caller's does.
    fn at(&mut self, at_ms: u64) {
        self.last_ms = self.last_ms.max(at_ms);
        self.clock.0.store(self.last_ms, Ordering::SeqCst);
    }

    fn card(&self) -> Option<Tarjeta> {
        self.motor.vista().tarjeta
    }

    /// Hands one input to the engine. Replaying goes through here too, so
    /// everything an input changes must happen in this function.
    fn feed(&mut self, at_ms: u64, input: &Input) -> Vec<Mensaje> {
        self.at(at_ms);
        let entry = match input.clone() {
            Input::Start => Entrada::Comenzar,
            Input::Text { text } => Entrada::Texto(text),
            Input::Graded { read, correct, wrong_form, explained, unreadable } => {
                self.held = None;
                Entrada::Calificada(Calificacion {
                    respuesta_leida: read,
                    respuesta_correcta: correct,
                    forma_incorrecta: wrong_form,
                    explicacion_correcta: explained,
                    // The engine speaks a comment as its own line. Only a model
                    // writes one, and no model grades here.
                    comentario: None,
                    ilegible: unreadable,
                })
            }
            Input::Help => Entrada::Ayuda,
            Input::Tired => Entrada::Cansado,
            Input::LeaveScreening => Entrada::SalirTamizaje,
            Input::Activity { choice } => Entrada::Actividad(choice),
            Input::Tick => Entrada::Tick,
            Input::Chat { text, intent } => Entrada::Chat { texto: text, intencion: intent },
            Input::Hold { said } => {
                self.held = self.card().map(|card| Held { card: card_key(&card), said });
                return Vec::new();
            }
        };
        self.motor.entrada(entry)
    }

    fn apply(&mut self, at_ms: u64, input: Input) -> Vec<Mensaje> {
        let messages = self.feed(at_ms, &input);
        self.unsaved.push(Record { at_ms: self.last_ms, input });
        messages
    }

    /// Applies an input and turns what the engine answers into lines. A line
    /// that answers the student's own words is marked as such.
    fn push(&mut self, at_ms: u64, input: Input, out: &mut Vec<Line>) {
        let echoes = matches!(input, Input::Text { .. } | Input::Chat { .. });
        let messages = self.apply(at_ms, input);
        lines_of(messages, echoes, out);
    }

    /// Lets the time that has passed act: a hint for an exercise that is
    /// taking long, the end of a break, the warning before the last
    /// exercises. Kept as an input only when it did something.
    fn tick(&mut self, at_ms: u64, out: &mut Vec<Line>) {
        self.at(at_ms);
        let before = self.motor.actividad();
        let messages = self.motor.entrada(Entrada::Tick);
        if !messages.is_empty() || self.motor.actividad() != before {
            self.unsaved.push(Record { at_ms: self.last_ms, input: Input::Tick });
        }
        lines_of(messages, false, out);
    }

    /// When the card on the screen changed, says what the engine only shows:
    /// the statement of a new exercise, and the drawing of a guided example.
    fn present(&self, before: Option<String>, out: &mut Vec<Line>) {
        let Some(card) = self.card() else { return };
        if Some(card_key(&card)) == before {
            return;
        }
        match card.modo {
            Modo::Tamizaje | Modo::Etapa2 | Modo::Tarea | Modo::Simulacion | Modo::Correccion => {
                out.push(card_line(&card));
            }
            // The engine's message already says the statement; its drawing is
            // only on the card.
            Modo::EjemploAsistido => {
                if let Some(drawing) = card.grafico.as_ref().and_then(visuals::drawing)
                    && let Some(line) = out.iter_mut().rev().find(|line| line.show.is_none())
                {
                    line.show = Some(drawing);
                }
            }
            _ => {}
        }
    }

    fn turn(&self, lines: Vec<Line>, reply: Reply) -> Turn {
        Turn { lines, converse: (reply == Reply::Free).then(|| self.rules()), closed: self.closed() }
    }

    /// What the tutor's model may and may not say now, from the engine.
    fn rules(&self) -> String {
        let context = self.motor.contexto_chat();
        let mut rules = format!("Momento de la sesión: {}.", context.momento);
        if let Some(statement) = &context.enunciado {
            rules.push_str(&format!("\nEn pantalla: {statement}"));
        }
        rules.push_str(&format!("\nReglas:\n- {}", context.reglas()));
        rules
    }

    /// Starts the engine and returns its opening lines. Asked again before
    /// anything else happened, it returns the same opening; later, where the
    /// session is, so a tutor that reconnects has something to pick up from.
    pub fn open(&mut self, at_ms: u64) -> Turn {
        if !self.started() {
            let mut lines = Vec::new();
            self.push(at_ms, Input::Start, &mut lines);
            self.present(None, &mut lines);
            return self.turn(lines, Reply::Scripted);
        }
        if self.stored + self.unsaved.len() == 1 {
            return Flow::new(self.setup.clone(), self.bank.clone()).open(0);
        }
        let lines = self.card().iter().map(card_line).collect();
        self.turn(lines, Reply::Scripted)
    }

    /// What the student said, or typed in the answer box of the page.
    ///
    /// Spoken words arrive as one text. They are read like this:
    ///
    /// 1. "I am tired" starts the engine's break, and asking to leave the
    ///    screening leaves it, whatever is awaited.
    /// 2. When an answer is awaited and the text holds one (a number, a
    ///    fraction, one of the options), it is graded with the engine's own
    ///    checker. The number is the answer (the one said before "porque"
    ///    or the end of the first sentence, when there is one there) and the
    ///    whole text is the explanation, checked with the engine's key words,
    ///    which name the numbers too. An answer whose explanation is missing
    ///    where one is required is held, the student is asked once how they
    ///    worked it out, and what they say next is added to it (or replaces
    ///    it, when it is said like a new answer).
    /// 3. "No sé" is a wrong answer where there are no hints (the screening
    ///    and the simulated exam) and a request for a hint elsewhere.
    /// 4. In the guided moments (a date, "shall we start?", the theory), text
    ///    that moves the flow goes to the engine as text.
    /// 5. Anything else is conversation: the engine is told its intent, and
    ///    the tutor's model replies under the engine's rules. Asking for help
    ///    or for the answer goes to the engine's hints instead.
    ///
    /// Typed text skips 1 and 5: the answer box holds an answer.
    pub fn said(&mut self, at_ms: u64, said: &str, explanation: Option<&str>, typed: bool) -> Turn {
        let before = self.card().as_ref().map(card_key);
        let stage = self.motor.actividad();
        let mut lines = Vec::new();
        self.tick(at_ms, &mut lines);
        // When time alone moved the session on (a break that ended), what was
        // said belongs to the moment before: it is not read as an answer to
        // an exercise the student has not heard yet.
        let reply = if self.motor.actividad() == stage {
            let explanation = explanation.map(str::trim).filter(|text| !text.is_empty());
            self.hear(at_ms, said.trim(), explanation, typed, &mut lines)
        } else {
            Reply::Scripted
        };
        self.present(before, &mut lines);
        self.turn(lines, reply)
    }

    fn hear(
        &mut self,
        at_ms: u64,
        said: &str,
        explanation: Option<&str>,
        typed: bool,
        out: &mut Vec<Line>,
    ) -> Reply {
        let awaited = self.motor.esperando();
        let intent = conversacion::clasificar(said);
        let text = || Input::Text { text: said.to_string() };
        if !typed {
            // In a break "tengo sueño" chooses an activity; it is not a request for another break.
            if intent == Intencion::EstoyCansado && !matches!(awaited, Esperando::Actividad { .. }) {
                self.push(at_ms, Input::Tired, out);
                return Reply::Scripted;
            }
            if self.motor.vista().boton_salir_tamizaje && wants_out(said) {
                self.push(at_ms, Input::LeaveScreening, out);
                return Reply::Scripted;
            }
        }
        match awaited {
            Esperando::Respuesta {
                respuesta,
                opciones,
                forma,
                pide_explicacion,
                claves,
                minimo_claves,
                ..
            } => {
                let key = Clave { respuesta: &respuesta, opciones: &opciones, forma };
                let needs = pide_explicacion.then_some((claves.as_slice(), minimo_claves));
                self.answer(at_ms, said, explanation, typed, intent, &key, needs, out)
            }
            Esperando::Explicacion { claves, minimo_claves, .. } => {
                // A long text is the explanation itself, even if a word in it sounds like a request.
                let short = said.split_whitespace().count() <= 6;
                if !typed && short && asks(intent, said) {
                    return self.chat(at_ms, said, intent, out);
                }
                if texto::es_no_se(said) {
                    self.push(at_ms, Input::Help, out);
                    return Reply::Scripted;
                }
                let explained =
                    correccion::explicacion_por_claves(explanation.unwrap_or(said), &claves, minimo_claves);
                self.push(at_ms, graded(None, explained, false, Some(explained), false), out);
                Reply::Scripted
            }
            Esperando::Conversacion { .. } => {
                if typed || self.motor.texto_es_flujo(said) {
                    self.push(at_ms, text(), out);
                    Reply::Scripted
                } else {
                    self.chat(at_ms, said, intent, out)
                }
            }
            Esperando::Actividad { .. } => {
                let chosen = |flow: &Self| flow.motor.vista().pausa.and_then(|pause| pause.actividad);
                let before = chosen(self);
                let messages = self.apply(at_ms, text());
                if typed || chosen(self) != before {
                    lines_of(messages, true, out);
                    return Reply::Scripted;
                }
                // The engine only said how long is left: let the model talk instead.
                self.chat(at_ms, said, intent, out)
            }
            Esperando::Nada => {
                self.push(at_ms, text(), out);
                Reply::Scripted
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn answer(
        &mut self,
        at_ms: u64,
        said: &str,
        explanation: Option<&str>,
        typed: bool,
        intent: Intencion,
        key: &Clave,
        needs: Option<(&[Vec<String>], usize)>,
        out: &mut Vec<Line>,
    ) -> Reply {
        let readable = |text: &str| {
            !has_long_number(text)
                && if key.opciones.is_empty() {
                    texto::leer_respuesta(text).is_some()
                } else {
                    texto::leer_opcion(text, key.opciones).is_some()
                }
        };
        let card = self.card().as_ref().map(card_key);
        let held =
            self.held.as_ref().filter(|held| Some(&held.card) == card.as_ref()).map(|h| h.said.clone());
        // After an answer that is held, what comes next is how it was worked
        // out, and that names numbers too. It is a new answer only when it
        // is said like one: a few words, a correction, or "... porque ...".
        let restated = typed || held.is_none() || {
            let words: Vec<String> = said.split_whitespace().map(texto::normalizar).collect();
            let corrects = ["no", "perdon", "mejor"].contains(&words.first().map_or("", String::as_str));
            words.len() <= 4 || corrects || answer_part(said, readable) != said
        };
        let fresh = restated && readable(said);
        // "¿Qué significa 3/4?" has a number in it and is still a question;
        // asking for help while giving an answer is an answer.
        let question = matches!(intent, Intencion::AclararPalabra | Intencion::RepetirEnunciado);
        if !typed && asks(intent, said) && (question || !fresh) {
            return self.chat(at_ms, said, intent, out);
        }
        let answer = match (&held, fresh) {
            (_, true) if typed => said.to_string(),
            (_, true) => answer_part(said, readable).to_string(),
            (Some(held), false) if typed => held.clone(),
            (Some(held), false) => answer_part(held, readable).to_string(),
            (None, false) => {
                if texto::es_no_se(said) {
                    let no_hints = self.motor.contexto_chat().sin_ayuda;
                    if no_hints && !typed && texto::normalizar(said).contains("no entiend") {
                        // Not giving up: asking. The rules allow explaining a word, not helping.
                        return self.chat(at_ms, said, intent, out);
                    }
                    let input =
                        if no_hints { graded(None, false, false, Some(false), false) } else { Input::Help };
                    self.push(at_ms, input, out);
                    return Reply::Scripted;
                }
                if typed {
                    self.push(at_ms, graded(None, false, false, None, true), out);
                    return Reply::Scripted;
                }
                return self.chat(at_ms, said, intent, out);
            }
        };
        let reading = correccion::corregir(key, &answer);
        let unreadable = reading.veredicto == Veredicto::Ilegible;
        let mut explained = None;
        if let (Some((keys, least)), false) = (needs, unreadable) {
            // Typed, the explanation has its own box. Spoken, it is everything
            // the student said about this exercise: the keys name the numbers too.
            let explanation = match (explanation, typed, &held) {
                (Some(explanation), ..) => explanation.to_string(),
                (None, true, _) => String::new(),
                (None, false, Some(held)) => format!("{held} {said}"),
                (None, false, None) => said.to_string(),
            };
            let enough = correccion::explicacion_por_claves(&explanation, keys, least);
            if !enough && held.is_none() {
                // Spoken, what was said so far is kept whole: it is the start of the explanation.
                let said = if typed { answer } else { said.to_string() };
                self.push(at_ms, Input::Hold { said }, out);
                out.push(Line::fixed(ASK_HOW, None));
                return Reply::Scripted;
            }
            explained = Some(enough);
        }
        let input = graded(
            reading.leida,
            reading.veredicto == Veredicto::Correcta,
            matches!(reading.veredicto, Veredicto::FormaIncorrecta { .. }),
            explained,
            unreadable,
        );
        self.push(at_ms, input, out);
        Reply::Scripted
    }

    /// Something said that is neither an answer nor a step of the flow.
    fn chat(&mut self, at_ms: u64, said: &str, intent: Intencion, out: &mut Vec<Line>) -> Reply {
        let chat = Input::Chat { text: said.to_string(), intent };
        match intent {
            // The engine's hints, or its fixed line where there are none.
            Intencion::PedirAyuda | Intencion::PedirRespuesta => {
                self.push(at_ms, Input::Help, out);
                Reply::Scripted
            }
            // Read again by the platform, word for word: no model needed.
            Intencion::RepetirEnunciado => {
                self.push(at_ms, chat, out);
                match self.card() {
                    Some(card) => {
                        out.push(card_line(&card));
                        Reply::Scripted
                    }
                    None => Reply::Free,
                }
            }
            _ => {
                let lines = out.len();
                self.push(at_ms, chat, out);
                // When the engine answers a chat itself, that is the whole turn.
                if out.len() > lines { Reply::Scripted } else { Reply::Free }
            }
        }
    }

    /// The session as a student's screen may show it. The engine's own view
    /// also carries the expected answer, the key words of the explanation and
    /// the criteria (`Esperando`), the rules it applied and the family's
    /// report: none of that is copied here, field by field on purpose.
    pub fn step(&self, at_ms: u64) -> Value {
        self.clock.0.store(self.last_ms.max(at_ms), Ordering::SeqCst);
        let view = self.motor.vista();
        self.clock.0.store(self.last_ms, Ordering::SeqCst);
        let route: Vec<Value> = view
            .ruta
            .iter()
            .map(|step| {
                let state = match step.estado.as_str() {
                    "hecho" => "done",
                    "actual" => "current",
                    _ => "pending",
                };
                json!({ "name": step.nombre, "detail": step.detalle, "state": state })
            })
            .collect();
        let card = view.tarjeta.map(|card| {
            json!({
                "mode": card.modo,
                "title": card.titulo,
                "statement": card.enunciado,
                "visual": card.grafico.as_ref().and_then(visuals::drawing),
                "options": card.opciones,
                "asks_answer": card.pide_respuesta,
                "asks_explanation": card.pide_explicacion,
                "is_exercise": card.es_ejercicio,
                "note": card.nota,
                "hints_allowed": card.permite_pistas,
                "format_help": card.ayuda_formato,
            })
        });
        json!({
            "stage": view.etapa,
            "progress": view.progreso,
            "route": route,
            "card": card,
            "elapsed_seconds": view.transcurrido_s,
            "closed": view.terminada,
        })
    }

    /// The family's report in the engine's words, one paragraph per message.
    /// Nothing while there is only the greeting: nothing happened to report.
    pub fn report(&self) -> Option<String> {
        let messages = self.motor.informe();
        (messages.len() > 1).then(|| messages.join("\n\n"))
    }
}

/// Whether the student is asking for something instead of answering. The
/// engine's classifier reads "creo que es..." and "así que son tres octavos"
/// as asking what a word means, so that one also has to look like a question.
fn asks(intent: Intencion, said: &str) -> bool {
    match intent {
        Intencion::RepetirEnunciado | Intencion::PedirAyuda | Intencion::PedirRespuesta => true,
        Intencion::AclararPalabra => is_question(said) || texto::normalizar(said).contains("significa"),
        _ => false,
    }
}

fn graded(
    read: Option<String>,
    correct: bool,
    wrong_form: bool,
    explained: Option<bool>,
    unreadable: bool,
) -> Input {
    Input::Graded { read, correct, wrong_form, explained, unreadable }
}

// ----------------------------------------------------------------- storage

/// A session's engine in this process.
pub struct Live {
    flow: Flow,
    /// The wall time of the engine's clock at zero.
    anchor: DateTime<Utc>,
    /// False while an input is being applied and stored. Left false by a
    /// failure, so the next call rebuilds the engine from what was stored.
    settled: bool,
    used: std::time::Instant,
}

/// The engines of the sessions running in this process.
pub type Flows = Arc<Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<Live>>>>>;

/// An engine nobody has used for this long belongs to a session that is over.
const FORGET_AFTER: std::time::Duration = std::time::Duration::from_secs(3 * 3600);

pub fn runs(session: &Session) -> bool {
    session.flow.as_deref() == Some(ENGINE)
}

/// For the routes of the free mode: the engine and the supervisor's
/// exercises cannot both decide what the student works on.
pub fn free(session: &Session) -> ApiResult<()> {
    if runs(session) { Err(ApiError::Conflict("the session is run by the session engine")) } else { Ok(()) }
}

fn by_engine(session: &Session) -> ApiResult<()> {
    if runs(session) {
        Ok(())
    } else {
        Err(ApiError::Conflict("the session is not run by the session engine"))
    }
}

async fn setup(state: &AppState, session: &Session) -> ApiResult<Setup> {
    let not_runnable = ApiError::Conflict("the session cannot be run by the session engine");
    let Some(child_id) = session.child_id else { return Err(not_runnable) };
    let child = find_child(&state.pool, child_id).await?;
    let guardian: Option<String> =
        sqlx::query_scalar("select name from guardians where family_id = $1 order by created_at limit 1")
            .bind(child.family_id)
            .fetch_optional(&state.pool)
            .await?;
    let objective: Option<Value> = sqlx::query_scalar("select objective from sessions where id = $1")
        .bind(session.id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let Some((goal, due)) = goal(objective.as_ref(), &session.language) else { return Err(not_runnable) };
    // The session's own day, so an engine rebuilt after midnight is the same one.
    let today = (session.created_at + Duration::minutes(i64::from(state.config.timezone_offset_minutes)))
        .date_naive();
    Ok(Setup {
        student: child.nickname,
        interests: child.interests,
        guardian: guardian.unwrap_or_else(|| GUARDIAN_FALLBACK.to_string()),
        today,
        goal,
        due,
    })
}

/// The session's engine rebuilt from its stored inputs.
async fn load(state: &AppState, session: &Session) -> ApiResult<Live> {
    let bank = bank().ok_or(ApiError::Unavailable("the session engine's bank is not valid"))?;
    let setup = setup(state, session).await?;
    let rows: Vec<(i64, sqlx::types::Json<Input>, DateTime<Utc>)> =
        sqlx::query_as("select at_ms, input, at from session_flow_inputs where session_id = $1 order by seq")
            .bind(session.id)
            .fetch_all(&state.pool)
            .await?;
    let anchor = rows.first().map_or_else(Utc::now, |(_, _, at)| *at);
    let records: Vec<Record> = rows
        .into_iter()
        .map(|(at_ms, input, _)| Record { at_ms: at_ms.max(0) as u64, input: input.0 })
        .collect();
    Ok(Live {
        flow: Flow::replay(setup, bank, &records),
        anchor,
        settled: true,
        used: std::time::Instant::now(),
    })
}

async fn live(state: &AppState, session: &Session) -> ApiResult<Arc<tokio::sync::Mutex<Live>>> {
    if let Some(found) = state.flows.lock().expect("lock").get(&session.id).cloned() {
        return Ok(found);
    }
    let built = Arc::new(tokio::sync::Mutex::new(load(state, session).await?));
    let mut flows = state.flows.lock().expect("lock");
    flows.retain(|_, live| live.try_lock().map_or(true, |live| live.used.elapsed() < FORGET_AFTER));
    Ok(flows.entry(session.id).or_insert(built).clone())
}

/// Drops the session's engine from this process, when the session is over.
pub fn forget(state: &AppState, session_id: Uuid) {
    state.flows.lock().expect("lock").remove(&session_id);
}

/// Stores the inputs applied since the last time. `queued` are lines the
/// tutor still has to be handed, kept with the last of them.
async fn save(
    pool: &PgPool,
    session_id: Uuid,
    live: &mut Live,
    queued: Option<&Value>,
) -> Result<(), sqlx::Error> {
    let records = live.flow.unsaved();
    if records.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (i, record) in records.iter().enumerate() {
        let last = i + 1 == records.len();
        sqlx::query(
            "insert into session_flow_inputs (session_id, seq, at_ms, input, queued, at)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(session_id)
        .bind((live.flow.stored() + i) as i32)
        .bind(record.at_ms as i64)
        .bind(sqlx::types::Json(&record.input))
        .bind(queued.filter(|_| last))
        .bind(live.anchor + Duration::milliseconds(record.at_ms as i64))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    live.flow.saved();
    Ok(())
}

/// Runs `act` on the session's engine at the current time and stores the
/// inputs it applied. One at a time per session.
async fn with_engine<T>(
    state: &AppState,
    session: &Session,
    act: impl FnOnce(&mut Flow, u64) -> ApiResult<(T, Option<Value>)>,
) -> ApiResult<T> {
    let live = live(state, session).await?;
    let mut live = live.lock().await;
    // Another process may have taken inputs, or the last call here failed half way.
    let stored: i64 = sqlx::query_scalar("select count(*) from session_flow_inputs where session_id = $1")
        .bind(session.id)
        .fetch_one(&state.pool)
        .await?;
    if !live.settled || stored != live.flow.stored() as i64 {
        *live = load(state, session).await?;
    }
    live.settled = false;
    live.used = std::time::Instant::now();
    if !live.flow.started() {
        live.anchor = Utc::now();
    }
    let at_ms = (Utc::now() - live.anchor).num_milliseconds().max(0) as u64;
    let (result, queued) = act(&mut live.flow, at_ms)?;
    save(&state.pool, session.id, &mut live, queued.as_ref()).await?;
    live.settled = true;
    Ok(result)
}

fn script(lines: &[Line], closed: bool) -> Value {
    json!({ "lines": lines, "closed": closed })
}

/// The engine's opening lines, for the tutor that just joined.
pub async fn open(state: &AppState, session: &Session) -> ApiResult<Value> {
    with_engine(state, session, |flow, at_ms| {
        let turn = flow.open(at_ms);
        Ok((script(&turn.lines, turn.closed), None))
    })
    .await
}

/// Lines from answers typed on the page since the tutor last asked, in
/// order. They are handed over once.
pub async fn queued(pool: &PgPool, session_id: Uuid) -> Result<Option<Value>, sqlx::Error> {
    let queued: Vec<sqlx::types::Json<Value>> = sqlx::query_scalar(
        "with due as (
             select seq, queued from session_flow_inputs
             where session_id = $1 and queued is not null for update
         ), handed as (
             update session_flow_inputs input set queued = null from due
             where input.session_id = $1 and input.seq = due.seq
         ) select queued from due order by seq",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    if queued.is_empty() {
        return Ok(None);
    }
    let mut lines = Vec::new();
    let mut closed = false;
    for part in queued {
        lines.extend(part.0["lines"].as_array().cloned().unwrap_or_default());
        closed |= part.0["closed"] == true;
    }
    Ok(Some(json!({ "lines": lines, "closed": closed })))
}

/// The family's report in the engine's words, when the engine ran the
/// session and something happened in it. Built from the stored inputs, so it
/// works in the worker, which never had the live engine.
pub async fn report(state: &AppState, session: &Session) -> ApiResult<Option<String>> {
    if !runs(session) {
        return Ok(None);
    }
    match load(state, session).await {
        Ok(live) => Ok(live.flow.report()),
        // The child is gone, or the engine cannot be rebuilt: there is no report, not a failed review.
        Err(ApiError::NotFound(_) | ApiError::Conflict(_) | ApiError::Unavailable(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The same delete as for a session's turns, for its inputs.
pub async fn delete_inputs_after_review(pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "delete from session_flow_inputs using sessions
         where session_flow_inputs.session_id = sessions.id and sessions.id = $1 and not sessions.keep_transcript",
    )
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------- handlers

fn checked(text: &str, what: &'static str) -> ApiResult<()> {
    if text.trim().is_empty() || text.chars().count() > MAX_SAID_CHARS {
        return Err(ApiError::bad_request(format!("{what} must have 1 to {MAX_SAID_CHARS} characters")));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct SaidInput {
    /// What the student said, in their words.
    said: String,
    /// How they worked it out, when the caller can tell it apart.
    #[serde(default)]
    explanation: Option<String>,
}

/// The agent reports what the student said and gets what the tutor does next.
pub async fn said(
    Agent(session): Agent,
    State(state): State<AppState>,
    Body(body): Body<SaidInput>,
) -> ApiResult<Json<Value>> {
    if session.status == "ended" {
        return Err(ApiError::Conflict("the session has ended"));
    }
    by_engine(&session)?;
    checked(&body.said, "said")?;
    if let Some(explanation) = body.explanation.as_deref().filter(|text| !text.trim().is_empty()) {
        checked(explanation, "explanation")?;
    }
    let turn = with_engine(&state, &session, |flow, at_ms| {
        // A tutor that reports speech before asking for the opening gets the
        // opening: what was said came before the engine had asked anything.
        if !flow.started() {
            return Ok((flow.open(at_ms), None));
        }
        Ok((flow.said(at_ms, &body.said, body.explanation.as_deref(), false), None))
    })
    .await?;
    Ok(Json(json!({ "lines": turn.lines, "converse": turn.converse, "closed": turn.closed })))
}

/// The session, for the operator or a guardian of its child.
async fn session_for(state: &AppState, caller: &Caller, session_id: Uuid) -> ApiResult<Session> {
    let session: Option<Session> = sqlx::query_as(
        "select id, child_id, audience, room, language, status, keep_transcript, use_memory, created_at, ended_at, flow
         from sessions where id = $1",
    )
    .bind(session_id)
    .fetch_optional(&state.pool)
    .await?;
    let session = session.ok_or(ApiError::NotFound("session"))?;
    match (caller, session.child_id) {
        // A session of another family does not exist as far as a guardian can tell.
        (_, Some(child_id)) => {
            child_for(&state.pool, caller, child_id).await.map_err(|_| ApiError::NotFound("session"))?;
        }
        (Caller::Operator, None) => {}
        (Caller::Guardian { .. }, None) => return Err(ApiError::NotFound("session")),
    }
    if session.status == "ended" {
        return Err(ApiError::Conflict("the session has ended"));
    }
    by_engine(&session)?;
    Ok(session)
}

/// Where the session is, for the student's screen.
pub async fn step(
    caller: Caller,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let session = session_for(&state, &caller, session_id).await?;
    Ok(Json(with_engine(&state, &session, |flow, at_ms| Ok((flow.step(at_ms), None))).await?))
}

#[derive(Deserialize)]
pub struct TypedInput {
    answer: String,
    #[serde(default)]
    explanation: Option<String>,
}

/// An answer typed on the page. The engine takes it like a spoken one; what
/// it answers is spoken by the tutor, which collects it with its next report
/// of turns. The page gets where the session is now.
pub async fn typed(
    caller: Caller,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Body(body): Body<TypedInput>,
) -> ApiResult<Json<Value>> {
    let session = session_for(&state, &caller, session_id).await?;
    checked(&body.answer, "answer")?;
    if let Some(explanation) = body.explanation.as_deref().filter(|text| !text.trim().is_empty()) {
        checked(explanation, "explanation")?;
    }
    let step = with_engine(&state, &session, |flow, at_ms| {
        if !flow.started() {
            return Err(ApiError::Conflict("the session has not started"));
        }
        let turn = flow.said(at_ms, &body.answer, body.explanation.as_deref(), true);
        Ok((flow.step(at_ms), Some(script(&turn.lines, turn.closed))))
    })
    .await?;
    Ok(Json(step))
}

#[cfg(test)]
mod tests;
