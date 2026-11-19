//! Núcleo de Aule: lógica pura, sin IO ni reloj real.
//! La máquina de estados de la sesión decide el flujo; la IA solo redacta el texto de Aule,
//! juzga explicaciones y lee fotos (eso vive en `aule-ai`).

pub mod banco;
pub mod config;
pub mod conversacion;
pub mod correccion;
pub mod expr;
pub mod formato;
pub mod fraccion;
pub mod informe;
pub mod metricas;
pub mod modelo;
pub mod motor;
pub mod reloj;
pub mod texto;
