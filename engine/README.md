# The session engine

What decides a tutoring session: which stage the student is in, which exercise
comes next, when a hint is due and when the session ends. It is a state machine
with no input or output of its own and no language model. A model only words
what the engine decided, judges an answer when asked and reads a photo of a
notebook. **The model never decides the flow.**

It was written for Aule's first web demo and is brought here as it was, so that
the tutor, the platform and the playground follow the same rules.

| Path | What |
|---|---|
| `crates/aule-core` | The engine (`motor.rs`), the model of a session, checking of answers, the report for the family. No IO |
| `crates/contenido` | The bank of exercises for fractions (5th grade, five levels) as TOML in `data/`, and the drawings as SVG |
| `docs/spec-demo-web.md` | The rules, each with an id (`TAM-02`, `E2-03`, `PIS-01`...). The code cites the id of the rule it implements |

## The flow

Objective of the session → **Tamizaje** (five questions, one per level, no hints)
→ for each level up to the target: **Etapa 1: Descubrimiento** (theory, worked
and assisted examples, the student explains it back) then **Etapa 2: Refuerzo**
(five exercises on their own; hints, then a step-by-step of a similar exercise)
→ the **Tarea**, the simulation of an evaluation or its correction → the report.

## Language

The code, its comments and its rules are in Spanish, unlike the rest of this
repository. The vocabulary is part of the specification (Alumno, Tamizaje,
Nivel objetivo, Pistas...) and is used as written.

## Running the tests

```bash
cd engine
cargo test --locked
```

The names in the bank and the tests are made up.
