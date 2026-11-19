//! Reloj inyectable. El núcleo nunca lee la hora real: el servidor web le pasa
//! un reloj real con "adelantar"; los tests usan `RelojManual`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub trait Reloj: Send + Sync {
    /// Milisegundos desde un origen arbitrario (monótono).
    fn ahora_ms(&self) -> u64;
}

/// Reloj que solo avanza cuando se le pide. Clonable: comparte el tiempo.
#[derive(Clone, Default)]
pub struct RelojManual(Arc<AtomicU64>);

impl RelojManual {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn avanzar_s(&self, s: u64) {
        self.0.fetch_add(s * 1000, Ordering::SeqCst);
    }
    pub fn avanzar_min(&self, m: u64) {
        self.avanzar_s(m * 60);
    }
}

impl Reloj for RelojManual {
    fn ahora_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
