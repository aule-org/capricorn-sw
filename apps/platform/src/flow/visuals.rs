//! The engine's drawings as visuals of `contracts/visuals/visual.v1.schema.json`.
//!
//! The engine draws its own SVG for a browser. Screens here draw from a
//! description, so each drawing is translated to the closest kind. A drawing
//! that has no faithful kind is not approximated: it becomes nothing, and the
//! caller shows the statement as text instead.

use aule_core::banco::Grafico;
use aule_core::config::ActividadPausa;
use aule_core::fraccion::Fraccion;
use aule_core::motor::Adjunto;
use serde_json::{Value, json};

// The limits of the contract. A drawing outside them is not sent cut down.
const MAX_FRACTIONS: usize = 4;
const MAX_DENOMINATOR: u32 = 12;
const MAX_NUMERATOR: u32 = 24;
const MAX_LABEL_CHARS: usize = 20;
const MAX_COUNTERS: u32 = 30;
const MAX_LINES: usize = 6;
const MAX_LINE_CHARS: usize = 60;
const MAX_TITLE_CHARS: usize = 80;
const MAX_CELL_CHARS: usize = 24;

/// A screen writes "3/8" next to a bar or a circle that has no label, which
/// is the answer when the question is which fraction is shaded. The engine
/// writes nothing there, and the contract has no way to say "no label", so an
/// unlabelled part carries a blank one.
const NO_LABEL: &str = " ";

fn part(shaded: u32, parts: u32, label: Option<&str>) -> Option<Value> {
    if parts == 0 || parts > MAX_DENOMINATOR || shaded > MAX_NUMERATOR {
        return None;
    }
    let label = match label.map(str::trim) {
        Some(label) if label.chars().count() > MAX_LABEL_CHARS => return None,
        Some(label) if !label.is_empty() => label,
        _ => NO_LABEL,
    };
    Some(json!({ "numerator": shaded, "denominator": parts, "label": label }))
}

/// The engine's drawing as a visual, or nothing when no kind draws it faithfully.
pub fn drawing(drawing: &Grafico) -> Option<Value> {
    match drawing {
        Grafico::Barra { partes, sombreadas, etiqueta } => Some(
            json!({ "kind": "fraction_bars", "fractions": [part(*sombreadas, *partes, etiqueta.as_deref())?] }),
        ),
        Grafico::Barras { filas } => {
            if filas.is_empty() || filas.len() > MAX_FRACTIONS {
                return None;
            }
            let fractions: Option<Vec<Value>> =
                filas.iter().map(|row| part(row.sombreadas, row.partes, row.etiqueta.as_deref())).collect();
            Some(json!({ "kind": "fraction_bars", "fractions": fractions? }))
        }
        Grafico::Circulo { partes, sombreadas } => {
            Some(json!({ "kind": "fraction_circle", "fractions": [part(*sombreadas, *partes, None)?] }))
        }
        // Several circles of the same parts, filled one after another: 7 fourths
        // are a whole circle and three fourths of the next.
        Grafico::Circulos { partes, sombreadas } => {
            if *partes == 0 {
                return None;
            }
            let circles = sombreadas.div_ceil(*partes).max(1);
            if circles as usize > MAX_FRACTIONS {
                return None;
            }
            let fractions: Option<Vec<Value>> = (0..circles)
                .map(|circle| part(sombreadas.saturating_sub(circle * partes).min(*partes), *partes, None))
                .collect();
            Some(json!({ "kind": "fraction_circle", "fractions": fractions? }))
        }
        // The engine's line names only the whole numbers and leaves the ticks
        // between them bare, to be counted. A screen writes every tick as a
        // decimal, which is not what the exercise shows, so only a line of
        // whole numbers is drawn.
        Grafico::Recta { desde, hasta, divisiones, marca, .. } => {
            if *divisiones != 1 || hasta <= desde {
                return None;
            }
            let mark =
                marca.as_deref().and_then(|mark| mark.parse::<Fraccion>().ok()).map(|mark| mark.a_f64());
            let mut line = json!({ "kind": "number_line", "from": desde, "to": hasta, "step": 1 });
            if let Some(mark) = mark {
                line["marks"] = json!([mark]);
            }
            Some(line)
        }
        Grafico::Conjunto { total, marcados, .. } => {
            if *total == 0 || *total > MAX_COUNTERS || marcados > total {
                return None;
            }
            let mut groups = Vec::new();
            if *marcados > 0 {
                groups.push(json!({ "count": marcados, "fill": "solid" }));
            }
            if total > marcados {
                groups.push(json!({ "count": total - marcados, "fill": "open" }));
            }
            Some(json!({ "kind": "counters", "groups": groups }))
        }
    }
}

/// Words put on lines no longer than a screen line. Nothing when the text
/// does not fit the screen: a statement is never shown cut.
fn wrap(text: &str) -> Option<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let length = word.chars().count();
        if length > MAX_LINE_CHARS {
            return None;
        }
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + length <= MAX_LINE_CHARS => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    (!lines.is_empty() && lines.len() <= MAX_LINES).then_some(lines)
}

/// A statement to read, as the `exercise` kind.
pub fn statement(title: Option<&str>, text: &str) -> Option<Value> {
    let mut visual = json!({ "kind": "exercise", "lines": wrap(text)? });
    if let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) {
        if title.chars().count() > MAX_TITLE_CHARS {
            return None;
        }
        visual["title"] = json!(title);
    }
    Some(visual)
}

/// The activities of a break, as a table: when each one helps, and its name.
fn activities(activities: &[ActividadPausa]) -> Option<Value> {
    let fits = |text: &str| !text.is_empty() && text.chars().count() <= MAX_CELL_CHARS;
    if activities.is_empty()
        || activities.len() > MAX_LINES
        || !activities.iter().all(|activity| fits(&activity.para) && fits(&activity.nombre))
    {
        return None;
    }
    let cells: Vec<Value> =
        activities.iter().map(|activity| json!([activity.para, activity.nombre])).collect();
    Some(json!({ "kind": "table", "headers": ["Si...", "Actividad"], "cells": cells }))
}

/// What a message of the engine carries for the screen.
pub fn attachment(attachment: &Adjunto) -> Option<Value> {
    match attachment {
        Adjunto::Grafico { grafico } => drawing(grafico),
        // A worked example: its drawing when it has one, otherwise its
        // statement. The steps are long sentences and are spoken, not shown.
        Adjunto::Pasos { titulo, enunciado, grafico, .. } => {
            grafico.as_ref().and_then(drawing).or_else(|| statement(Some(titulo), enunciado))
        }
        // The names of the steps the student is about to follow, one per line.
        Adjunto::Secuencia { pasos } => {
            let fits =
                pasos.iter().all(|step| !step.is_empty() && step.chars().count() <= MAX_LINE_CHARS - 3);
            (fits && !pasos.is_empty() && pasos.len() <= MAX_LINES).then(|| {
                let lines: Vec<String> =
                    pasos.iter().enumerate().map(|(i, step)| format!("{}. {step}", i + 1)).collect();
                json!({ "kind": "exercise", "lines": lines })
            })
        }
        Adjunto::Actividades { actividades } => activities(actividades),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aule_core::banco::{Banco, Ejercicio, FilaBarra};

    /// The contract every visual must follow, compiled once per test.
    pub(crate) fn contract() -> jsonschema::Validator {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/visuals/visual.v1.schema.json");
        let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        jsonschema::validator_for(&schema).unwrap()
    }

    pub(crate) fn assert_follows(contract: &jsonschema::Validator, visual: &Value) {
        let errors: Vec<String> = contract.iter_errors(visual).map(|error| error.to_string()).collect();
        assert!(errors.is_empty(), "{visual}: {errors:?}");
    }

    /// Every drawing in the bank, with where it is used.
    fn drawings(bank: &Banco) -> Vec<Grafico> {
        fn of_exercise(exercise: &Ejercicio, all: &mut Vec<Grafico>) {
            all.extend(exercise.grafico.clone());
            all.extend(exercise.pista_grafica.as_ref().map(|hint| hint.grafico.clone()));
            all.extend(exercise.similar.as_ref().and_then(|similar| similar.grafico.clone()));
        }
        let mut all = Vec::new();
        for level in &bank.niveles {
            for exercise in level.tamizaje.iter().chain(&level.ejercicios).chain(&level.simulacion) {
                of_exercise(exercise, &mut all);
            }
            let discovery = &level.etapa1;
            all.extend(
                discovery.teoria.iter().chain(&discovery.repaso).filter_map(|block| block.grafico.clone()),
            );
            all.extend(discovery.no_asistidos.iter().filter_map(|example| example.grafico.clone()));
            all.extend(discovery.asistidos.iter().filter_map(|example| example.grafico.clone()));
            all.push(discovery.explicar.pista_grafica.grafico.clone());
        }
        for homework in &bank.tareas {
            for exercise in &homework.ejercicios {
                of_exercise(exercise, &mut all);
            }
        }
        all
    }

    #[test]
    fn every_drawing_of_the_bank_follows_the_contract_or_is_left_out() {
        let bank = contenido::cargar_validado().unwrap();
        let contract = contract();
        let all = drawings(&bank);
        assert!(all.len() > 100, "the bank has drawings: {}", all.len());
        let mut drawn = 0;
        for one in &all {
            match drawing(one) {
                Some(visual) => {
                    assert_follows(&contract, &visual);
                    drawn += 1;
                }
                // Only what the contract cannot draw is left out.
                None => assert!(
                    matches!(one, Grafico::Recta { .. } | Grafico::Barras { .. } | Grafico::Barra { .. }),
                    "{one:?}"
                ),
            }
        }
        assert!(drawn * 10 >= all.len() * 8, "most drawings have a kind: {drawn} of {}", all.len());
    }

    #[test]
    fn every_statement_of_the_bank_fits_the_screen_as_text() {
        let bank = contenido::cargar_validado().unwrap();
        let contract = contract();
        for level in &bank.niveles {
            for exercise in level.tamizaje.iter().chain(&level.ejercicios).chain(&level.simulacion) {
                let visual = statement(None, &exercise.enunciado).expect("fits in six lines");
                assert_follows(&contract, &visual);
            }
        }
    }

    #[test]
    fn a_shaded_shape_never_has_its_fraction_written_next_to_it() {
        // "What fraction is shaded?" must not be answered by the label.
        for visual in [
            drawing(&Grafico::Circulo { partes: 8, sombreadas: 3 }).unwrap(),
            drawing(&Grafico::Barra { partes: 5, sombreadas: 2, etiqueta: None }).unwrap(),
            drawing(&Grafico::Circulos { partes: 4, sombreadas: 7 }).unwrap(),
        ] {
            for fraction in visual["fractions"].as_array().unwrap() {
                assert_eq!(fraction["label"], NO_LABEL, "{visual}");
            }
        }
        // A label the bank wrote is kept: it is part of the drawing.
        let labelled = drawing(&Grafico::Barras {
            filas: vec![FilaBarra { partes: 4, sombreadas: 3, etiqueta: Some("3/4".into()) }],
        })
        .unwrap();
        assert_eq!(labelled["fractions"][0]["label"], "3/4");
    }

    #[test]
    fn shapes_are_translated_part_by_part() {
        let contract = contract();
        let circles = drawing(&Grafico::Circulos { partes: 4, sombreadas: 7 }).unwrap();
        assert_follows(&contract, &circles);
        let shaded: Vec<u64> = circles["fractions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["numerator"].as_u64().unwrap())
            .collect();
        assert_eq!(shaded, [4, 3]);

        let set = drawing(&Grafico::Conjunto { total: 12, marcados: 5, icono: None }).unwrap();
        assert_follows(&contract, &set);
        assert_eq!(set["groups"], json!([{ "count": 5, "fill": "solid" }, { "count": 7, "fill": "open" }]));

        // A line cut into fourths has no faithful kind; one of whole numbers has.
        let fourths =
            Grafico::Recta { desde: 0, hasta: 3, divisiones: 4, marca: Some("9/4".into()), pregunta: true };
        assert_eq!(drawing(&fourths), None);
        let whole =
            Grafico::Recta { desde: 0, hasta: 5, divisiones: 1, marca: Some("3".into()), pregunta: false };
        assert_follows(&contract, &drawing(&whole).unwrap());

        // Outside what a screen can draw: nothing, never a cut-down drawing.
        assert_eq!(drawing(&Grafico::Barra { partes: 24, sombreadas: 5, etiqueta: None }), None);
    }

    #[test]
    fn what_a_message_carries_follows_the_contract() {
        let contract = contract();
        let pause =
            attachment(&Adjunto::Actividades { actividades: aule_core::config::actividades_propuestas() })
                .unwrap();
        assert_follows(&contract, &pause);
        assert_eq!(pause["kind"], "table");

        let steps = attachment(&Adjunto::Secuencia {
            pasos: vec!["Contar todas las partes".into(), "Escribir la fracción".into()],
        })
        .unwrap();
        assert_follows(&contract, &steps);
        assert_eq!(steps["lines"][1], "2. Escribir la fracción");

        let example = attachment(&Adjunto::Pasos {
            titulo: "Ejemplo resuelto".into(),
            enunciado: "¿Qué fracción de la barra está pintada?".into(),
            pasos: vec!["Cuento las partes.".into()],
            grafico: None,
        })
        .unwrap();
        assert_follows(&contract, &example);
        assert_eq!(example["title"], "Ejemplo resuelto");

        // Too long for the screen: not shown at all.
        assert_eq!(statement(None, &"palabra ".repeat(60)), None);
    }
}
