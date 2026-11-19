//! Ayuda de formato del campo de respuesta ("cómo se escribe"). Los ejemplos se
//! eligen por ejercicio para que nunca coincidan con la respuesta (en ningún forma
//! equivalente: simplificada, mixta, impropia) ni usen los números del enunciado:
//! así nadie confunde el ejemplo con "Aule me dio la respuesta".

use crate::fraccion::Fraccion;

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

fn valor(s: &str) -> Option<Fraccion> {
    s.parse::<Fraccion>().ok()
}

/// Ejemplos (fracción, mixto, entero) que no chocan con la respuesta ni con los textos en pantalla.
/// Se buscan del más sencillo al más complejo: el primero que no use ningún número del
/// enunciado ni de la respuesta y que no valga lo mismo que la respuesta.
pub fn ejemplos(respuesta: &str, textos: &[&str]) -> (String, String, String) {
    let mut prohibidos: Vec<String> = textos.iter().flat_map(|t| numeros(t)).collect();
    prohibidos.extend(numeros(respuesta));
    let r = valor(respuesta);
    let libre = |n: i64| !prohibidos.contains(&n.to_string());
    let sirve = |txt: &str| valor(txt) != r || r.is_none();
    let mut fraccion = None;
    'f: for den in 3..60i64 {
        for num in 1..den {
            let t = format!("{num}/{den}");
            if libre(num) && libre(den) && crate::fraccion::mcd(num, den) == 1 && sirve(&t) {
                fraccion = Some(t);
                break 'f;
            }
        }
    }
    // Los siguientes ejemplos tampoco repiten los números del primero (más variados).
    if let Some(fr) = &fraccion {
        prohibidos.extend(numeros(fr));
    }
    let libre = |n: i64| !prohibidos.contains(&n.to_string());
    let mut mixto = None;
    'm: for den in 3..60i64 {
        for ent in 2..30i64 {
            for num in 1..den {
                let t = format!("{ent} {num}/{den}");
                if libre(ent)
                    && libre(num)
                    && libre(den)
                    && crate::fraccion::mcd(num, den) == 1
                    && sirve(&t)
                {
                    mixto = Some(t);
                    break 'm;
                }
            }
        }
    }
    if let Some(mx) = &mixto {
        prohibidos.extend(numeros(mx));
    }
    let entero = (7..200i64)
        .map(|n| n.to_string())
        .find(|t| !prohibidos.contains(t) && sirve(t))
        .unwrap_or_else(|| "250".into());
    (
        fraccion.unwrap_or_else(|| "97/101".into()),
        mixto.unwrap_or_else(|| "88 97/101".into()),
        entero,
    )
}

/// Línea de ayuda de formato, sin métodos ni pasos para resolver.
pub fn ayuda(respuesta: &str, textos: &[&str]) -> String {
    let (f, m, e) = ejemplos(respuesta, textos);
    format!(
        "Cómo se escribe: una fracción con barra (por ejemplo, {f}), un número mixto con un espacio (por ejemplo, {m}) o un número entero (por ejemplo, {e})."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_usa_la_respuesta_ni_el_enunciado() {
        let (f, m, e) = ejemplos("3/4", &["Simplifica 6/8 hasta su forma más simple."]);
        for x in [&f, &m, &e] {
            assert_ne!(valor(x), valor("3/4"));
            assert!(
                !numeros(x)
                    .iter()
                    .any(|n| ["6", "8", "3", "4"].contains(&n.as_str())),
                "{x}"
            );
        }
        // Respuesta que coincide con un candidato: se salta.
        let (f, _, _) = ejemplos("1/3", &[]);
        assert_ne!(f, "1/3");
        let (_, _, e2) = ejemplos("2", &[]);
        assert_ne!(e2, "2");
        // Equivalentes: 4/10 = 2/5 no se usa como "2/5".
        let (f, _, _) = ejemplos("4/10", &[]);
        assert_ne!(valor(&f), valor("4/10"));
    }
}
