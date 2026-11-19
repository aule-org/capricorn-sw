//! Ninguna ayuda de formato puede parecer la respuesta ni insinuar el método.

use aule_core::banco::Ejercicio;
use aule_core::formato::{ayuda, ejemplos};
use aule_core::fraccion::Fraccion;

const PALABRAS_DE_METODO: [&str; 12] = [
    "divid",
    "simplific",
    "multiplic",
    "sum",
    "rest",
    "común denominador",
    "comun denominador",
    "mcm",
    "equivalent",
    "numerador",
    "denominador",
    "convierte",
];

fn numeros(s: &str) -> Vec<String> {
    let mut v = vec![];
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            v.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        v.push(cur);
    }
    v
}

fn revisar(id: &str, respuesta: &str, textos: &[String], errores: &mut Vec<String>) {
    let refs: Vec<&str> = textos.iter().map(|s| s.as_str()).collect();
    let (f, m, e) = ejemplos(respuesta, &refs);
    let r = respuesta.parse::<Fraccion>().ok();
    let usados: Vec<String> = textos
        .iter()
        .flat_map(|t| numeros(t))
        .chain(numeros(respuesta))
        .collect();
    for x in [&f, &m, &e] {
        if x.parse::<Fraccion>().ok() == r {
            errores.push(format!(
                "{id}: el ejemplo {x} vale lo mismo que la respuesta {respuesta}"
            ));
        }
        if numeros(x).iter().any(|n| usados.contains(n)) {
            errores.push(format!(
                "{id}: el ejemplo {x} usa un número del enunciado o de la respuesta"
            ));
        }
    }
    let linea = ayuda(respuesta, &refs).to_lowercase();
    for p in PALABRAS_DE_METODO {
        if linea.contains(p) {
            errores.push(format!("{id}: la ayuda menciona un método ({p})"));
        }
    }
}

fn textos_de(e: &Ejercicio) -> Vec<String> {
    let mut v = vec![e.enunciado.clone()];
    v.extend(e.contextos.values().cloned());
    v
}

#[test]
fn ayuda_de_formato_nunca_es_la_respuesta() {
    let b = contenido::cargar_validado().unwrap();
    let mut errores = vec![];
    let mut todos: Vec<&Ejercicio> = vec![];
    for n in &b.niveles {
        todos.extend(n.tamizaje.iter());
        todos.extend(n.ejercicios.iter());
        todos.extend(n.simulacion.iter());
        for x in &n.etapa1.asistidos {
            for (k, p) in x.pasos.iter().enumerate() {
                if p.opciones.is_empty() {
                    revisar(
                        &format!("nivel {} asistido paso {k}", n.nivel),
                        &p.respuesta,
                        &[x.enunciado.clone(), p.indicacion.clone()],
                        &mut errores,
                    );
                }
            }
        }
    }
    for t in &b.tareas {
        todos.extend(t.ejercicios.iter());
    }
    for ev in &b.evaluaciones {
        todos.extend(ev.preguntas.iter().map(|p| &p.ejercicio));
    }
    let mut revisados = 0;
    for e in todos {
        if e.opciones.is_empty() {
            revisar(&e.id, &e.respuesta, &textos_de(e), &mut errores);
            revisados += 1;
        }
    }
    assert!(revisados > 50);
    assert!(errores.is_empty(), "{}", errores.join("\n"));
}
