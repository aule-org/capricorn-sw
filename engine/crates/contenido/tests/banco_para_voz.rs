//! El banco se dice en voz alta: ningún texto pide escribir, apretar o hacer
//! clic, ni habla de una duración de sesión o de un enlace. "Cómo se escribe"
//! enseña la notación y sí vale.

use std::fs;

const NO_PARA_VOZ: [&str; 12] = [
    "escribe",
    "escríbe",
    "escríbel",
    "escribir",
    "aprieta",
    "presiona",
    "haz clic",
    "botón",
    "recuadro",
    "whatsapp",
    "hora y media",
    "http",
];

#[test]
fn ningun_texto_del_banco_pide_escribir_ni_apretar() {
    let carpeta = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
    let mut marcados = Vec::new();
    for entrada in fs::read_dir(carpeta).expect("la carpeta del banco") {
        let ruta = entrada.expect("un archivo").path();
        if ruta.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let texto = fs::read_to_string(&ruta).expect("un archivo legible");
        for (n, linea) in texto.lines().enumerate() {
            let baja = linea
                .to_lowercase()
                .replace("se escribe", "")
                .replace("leer, escribir", "");
            if let Some(palabra) = NO_PARA_VOZ.iter().find(|p| baja.contains(*p)) {
                marcados.push(format!("{}:{}: {palabra}", ruta.display(), n + 1));
            }
        }
    }
    assert!(
        marcados.is_empty(),
        "textos que no sirven en una sesión por voz:\n{}",
        marcados.join("\n")
    );
}
