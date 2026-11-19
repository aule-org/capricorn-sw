//! MET-01 Next item correctness y MET-04 Time-to-master.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Contexto {
    Etapa2,
    Simulacion,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistroNic {
    pub tema: String,
    pub nivel: u8,
    pub contexto: Contexto,
    /// Por ejercicio: resuelto bien al primer intento y sin pistas.
    pub primer_intento: Vec<bool>,
}

impl RegistroNic {
    pub fn aciertos(&self) -> usize {
        self.primer_intento.iter().filter(|x| **x).count()
    }
    pub fn porcentaje(&self) -> f64 {
        if self.primer_intento.is_empty() {
            0.0
        } else {
            100.0 * self.aciertos() as f64 / self.primer_intento.len() as f64
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistroTtm {
    pub tema: String,
    pub nivel: u8,
    pub minutos: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metricas {
    pub nic: Vec<RegistroNic>,
    pub ttm: Vec<RegistroTtm>,
}
