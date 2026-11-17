//! Evaluates and refits `skill_model` from a log of answers, without an LLM.
//!
//! ```sh
//! cargo run --release --example skill_model_eval -- answers.tsv [--no-time]
//! ```
//!
//! The file has one answer per line, no header, tab-separated: `student`,
//! `skill`, `question`, `correct` (0/1), `Unix seconds`, `choice` (0/1). With
//! `--no-time` the fifth column only gives the order and forgetting is left
//! out of the fit.
//!
//! Protocol: each answer is predicted before it is used to update the model,
//! in date order. The parameters are fit on 30% of the students and the
//! metrics are reported on the remaining 70%. It compares against a running
//! mean and removes parts of the model one at a time.

use std::collections::HashMap;

use capricorn_platform::learning::skill_model::{Calibration, Params};
use chrono::{DateTime, Utc};

struct Row {
    student: u32,
    skill: u32,
    item: u32,
    correct: bool,
    at: i64,
    choice: bool,
}

/// Answers in date order and the names behind each column of ids.
struct Data {
    rows: Vec<Row>,
    names: [Vec<String>; 3],
}

fn load(path: &str) -> Data {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let mut ids: [HashMap<String, u32>; 3] = Default::default();
    let mut names: [Vec<String>; 3] = Default::default();
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        assert!(fields.len() >= 6, "line {}: expected 6 columns", number + 1);
        let mut intern = |kind: usize| {
            *ids[kind].entry(fields[kind].to_string()).or_insert_with(|| {
                names[kind].push(fields[kind].to_string());
                names[kind].len() as u32 - 1
            })
        };
        rows.push(Row {
            student: intern(0),
            skill: intern(1),
            item: intern(2),
            correct: fields[3] == "1",
            at: fields[4].parse().unwrap_or_else(|_| panic!("line {}: invalid timestamp", number + 1)),
            choice: fields[5] == "1",
        });
    }
    rows.sort_by_key(|row| row.at);
    Data { rows, names }
}

/// Students the parameters are fit on (3 out of 10).
fn fits(student: u32) -> bool {
    student.wrapping_mul(2_654_435_761) % 10 < 3
}

fn time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("timestamp out of range")
}

/// A model that predicts an answer and then learns from it.
trait Tracer {
    fn step(&mut self, row: &Row, data: &Data) -> f64;
}

struct RunningMean {
    correct: f64,
    total: f64,
}

impl Tracer for RunningMean {
    fn step(&mut self, row: &Row, _data: &Data) -> f64 {
        let p = (self.correct + 1.0) / (self.total + 2.0);
        self.correct += f64::from(u8::from(row.correct));
        self.total += 1.0;
        p
    }
}

impl Tracer for Calibration {
    fn step(&mut self, row: &Row, data: &Data) -> f64 {
        let name = |kind: usize, id: u32| data.names[kind][id as usize].as_str();
        let credit = f64::from(u8::from(row.correct));
        self.observe(
            name(0, row.student),
            name(1, row.skill),
            name(2, row.item),
            row.choice,
            credit,
            time(row.at),
        )
    }
}

#[derive(Default)]
struct Scores {
    predicted: Vec<f64>,
    correct: Vec<bool>,
}

impl Scores {
    fn push(&mut self, predicted: f64, correct: bool) {
        self.predicted.push(predicted.clamp(1e-6, 1.0 - 1e-6));
        self.correct.push(correct);
    }

    fn log_loss(&self) -> f64 {
        let sum: f64 = self
            .predicted
            .iter()
            .zip(&self.correct)
            .map(|(p, y)| if *y { -p.ln() } else { -(1.0 - p).ln() })
            .sum();
        sum / self.predicted.len() as f64
    }

    fn rmse(&self) -> f64 {
        let sum: f64 = self
            .predicted
            .iter()
            .zip(&self.correct)
            .map(|(p, y)| (p - f64::from(u8::from(*y))).powi(2))
            .sum();
        (sum / self.predicted.len() as f64).sqrt()
    }

    /// Area under the ROC curve by rank sum (mean rank on ties).
    fn auc(&self) -> f64 {
        let mut order: Vec<usize> = (0..self.predicted.len()).collect();
        order.sort_by(|a, b| self.predicted[*a].total_cmp(&self.predicted[*b]));
        let (mut rank_sum, mut positives, mut start) = (0.0, 0.0, 0);
        while start < order.len() {
            let mut end = start;
            while end < order.len() && self.predicted[order[end]] == self.predicted[order[start]] {
                end += 1;
            }
            let rank = (start + end + 1) as f64 / 2.0;
            let hits = order[start..end].iter().filter(|index| self.correct[**index]).count() as f64;
            rank_sum += rank * hits;
            positives += hits;
            start = end;
        }
        let negatives = order.len() as f64 - positives;
        (rank_sum - positives * (positives + 1.0) / 2.0) / (positives * negatives)
    }

    /// (mean prediction, observed success rate, answers) in 10 bins.
    fn calibration(&self) -> Vec<(f64, f64, usize)> {
        let mut bins = vec![(0.0, 0.0, 0usize); 10];
        for (p, y) in self.predicted.iter().zip(&self.correct) {
            let bin = &mut bins[((p * 10.0) as usize).min(9)];
            *bin = (bin.0 + p, bin.1 + f64::from(u8::from(*y)), bin.2 + 1);
        }
        bins.into_iter().filter(|bin| bin.2 > 0).map(|(p, y, n)| (p / n as f64, y / n as f64, n)).collect()
    }

    /// Expected calibration error: mean gap between predicted and observed.
    fn ece(&self) -> f64 {
        let total = self.predicted.len() as f64;
        self.calibration().iter().map(|(p, y, n)| (p - y).abs() * *n as f64 / total).sum()
    }
}

/// Metrics of one pass: all answers, and the first three of each student on
/// a skill.
#[derive(Default)]
struct Report {
    all: Scores,
    cold: Scores,
}

fn run(data: &Data, tracer: &mut dyn Tracer, scored: impl Fn(u32) -> bool, only_scored: bool) -> Report {
    let mut report = Report::default();
    let mut seen: HashMap<(u32, u32), u32> = HashMap::new();
    for row in &data.rows {
        let counts = scored(row.student);
        if only_scored && !counts {
            continue;
        }
        let predicted = tracer.step(row, data);
        let attempts = seen.entry((row.student, row.skill)).or_insert(0);
        if counts {
            report.all.push(predicted, row.correct);
            if *attempts < 3 {
                report.cold.push(predicted, row.correct);
            }
        }
        *attempts += 1;
    }
    report
}

/// Coordinate descent over grids: tries each value of each parameter with the
/// others fixed and keeps the one with the lowest loss.
fn coordinate_search(mut values: Vec<f64>, grids: &[&[f64]], loss: impl Fn(&[f64]) -> f64) -> Vec<f64> {
    let mut best = loss(&values);
    for _ in 0..3 {
        let before = best;
        for (index, grid) in grids.iter().enumerate() {
            for candidate in grid.iter() {
                let previous = std::mem::replace(&mut values[index], *candidate);
                let current = loss(&values);
                if current < best {
                    best = current;
                } else {
                    values[index] = previous;
                }
            }
        }
        if before - best < 1e-5 {
            break;
        }
    }
    values
}

fn elo_params(values: &[f64]) -> Params {
    Params {
        prior_ability: values[0],
        global_step: values[1],
        global_decay: values[2],
        skill_step: values[3],
        skill_decay: values[4],
        practice_gain: values[5],
        item_step: values[6],
        item_decay: values[7],
        guess_open: values[8],
        guess_choice: values[9],
        forget_weight: values[10],
        half_life_init_days: values[11],
        half_life_growth: values[12],
        half_life_shrink: values[13],
        ..Params::default()
    }
}

fn elo_values(params: &Params) -> Vec<f64> {
    vec![
        params.prior_ability,
        params.global_step,
        params.global_decay,
        params.skill_step,
        params.skill_decay,
        params.practice_gain,
        params.item_step,
        params.item_decay,
        params.guess_open,
        params.guess_choice,
        params.forget_weight,
        params.half_life_init_days,
        params.half_life_growth,
        params.half_life_shrink,
    ]
}

fn row(name: &str, report: &Report) {
    println!(
        "| {name} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |",
        report.all.auc(),
        report.all.log_loss(),
        report.all.rmse(),
        report.all.ece(),
        report.cold.auc(),
        report.cold.log_loss()
    );
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: skill_model_eval <answers.tsv> [--no-time]");
    let timeless = std::env::args().any(|arg| arg == "--no-time");
    let data = load(&path);
    println!(
        "{} answers, {} students ({} for fitting), {} skills, {} questions, mean success rate {:.3}\n",
        data.rows.len(),
        data.names[0].len(),
        (0..data.names[0].len() as u32).filter(|student| fits(*student)).count(),
        data.names[1].len(),
        data.names[2].len(),
        data.rows.iter().filter(|row| row.correct).count() as f64 / data.rows.len() as f64
    );

    // Without real timestamps there is no forgetting to fit: an empty grid keeps the initial value.
    let timed = |grid: &'static [f64]| -> &'static [f64] { if timeless { &[] } else { grid } };
    let steps: &[f64] = &[0.0, 0.05, 0.1, 0.2, 0.3, 0.4, 0.6, 0.8, 1.0, 1.5];
    let decays: &[f64] = &[0.0, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5];
    let guesses: &[f64] = &[0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3];
    let elo_grids: [&[f64]; 14] = [
        &[-1.0, -0.5, -0.25, 0.0, 0.25, 0.5, 1.0],
        steps,
        decays,
        steps,
        decays,
        &[0.0, 0.05, 0.1, 0.2, 0.3, 0.5],
        steps,
        decays,
        guesses,
        guesses,
        timed(&[0.0, 0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0]),
        timed(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0]),
        timed(&[0.0, 0.5, 1.0, 2.0, 4.0, 8.0]),
        timed(&[0.25, 0.5, 0.75, 1.0]),
    ];
    let defaults = Params {
        forget_weight: if timeless { 0.0 } else { Params::default().forget_weight },
        ..Params::default()
    };
    let fit_loss = |tracer: &mut dyn Tracer| run(&data, tracer, fits, true).all.log_loss();
    let fitted = elo_params(&coordinate_search(elo_values(&defaults), &elo_grids, |values| {
        fit_loss(&mut Calibration::new(elo_params(values)))
    }));

    let test = |student: u32| !fits(student);
    println!("| Model | AUC | Log loss | RMSE | ECE | AUC (first 3) | Log loss (first 3) |");
    println!("|---|---|---|---|---|---|---|");
    row("Running mean", &run(&data, &mut RunningMean { correct: 0.0, total: 0.0 }, test, false));
    row("Elo with memory, defaults", &run(&data, &mut Calibration::new(defaults), test, false));
    let full = run(&data, &mut Calibration::new(fitted), test, false);
    row("Elo with memory, refit", &full);
    let ablations = [
        ("  no forgetting", Params { forget_weight: 0.0, ..fitted }, true),
        ("  no practice gain", Params { practice_gain: 0.0, ..fitted }, true),
        ("  no per-skill difficulty", fitted, false),
        ("  no difficulty at all", Params { item_step: 0.0, ..fitted }, true),
        ("  no global ability", Params { global_step: 0.0, ..fitted }, true),
        ("  no per-skill offset", Params { skill_step: 0.0, ..fitted }, true),
    ];
    for (name, params, by_skill) in ablations {
        row(name, &run(&data, &mut Calibration { by_skill, ..Calibration::new(params) }, test, false));
    }

    println!("\nCalibration of \"Elo with memory, refit\" (predicted → observed, answers):");
    for (predicted, observed, count) in full.all.calibration() {
        println!("  {predicted:.3} → {observed:.3}  ({count})");
    }
    println!("\nRefit parameters:\n{}", serde_json::to_string_pretty(&fitted).unwrap());
}
