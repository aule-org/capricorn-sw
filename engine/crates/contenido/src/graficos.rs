//! Render de gráficos a SVG (barras de fracción, círculos, recta numérica, conjuntos).
//! Estilo "papel": trazo negro, sombreado gris oscuro con trama, alto contraste.

use aule_core::banco::Grafico;
use aule_core::fraccion::Fraccion;
use std::f64::consts::PI;
use std::fmt::Write;

const TINTA: &str = "#1b1b1b";
const SOMBRA: &str = "#4a4a4a";
const PAPEL: &str = "#fbfaf6";

/// Cada SVG lleva su propio id de trama: con ids repetidos, el navegador usa el
/// primero del documento y, si ese está oculto, las barras salen sin sombreado.
static CONTADOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn defs(id: &str) -> String {
    format!(
        r##"<defs><pattern id="{id}" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="6" height="6" fill="{SOMBRA}"/><line x1="0" y1="0" x2="0" y2="6" stroke="#2c2c2c" stroke-width="2"/></pattern></defs>"##
    )
}

fn envolver(w: f64, h: f64, cuerpo: &str, titulo: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="{titulo}" class="grafico">{}{cuerpo}</svg>"#,
        defs("trama")
    )
}

fn barra(x: f64, y: f64, w: f64, h: f64, partes: u32, sombreadas: u32) -> String {
    let mut s = String::new();
    let p = partes.max(1) as f64;
    let cw = w / p;
    for i in 0..partes.max(1) {
        let fill = if i < sombreadas { "url(#trama)" } else { PAPEL };
        let _ = write!(
            s,
            r#"<rect x="{:.2}" y="{y}" width="{:.2}" height="{h}" fill="{fill}" stroke="{TINTA}" stroke-width="2"/>"#,
            x + cw * i as f64,
            cw
        );
    }
    s
}

fn circulo(cx: f64, cy: f64, r: f64, partes: u32, sombreadas: u32) -> String {
    let mut s = String::new();
    let p = partes.max(1);
    if p == 1 {
        let fill = if sombreadas >= 1 {
            "url(#trama)"
        } else {
            PAPEL
        };
        let _ = write!(
            s,
            r#"<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}" stroke="{TINTA}" stroke-width="2"/>"#
        );
        return s;
    }
    for i in 0..p {
        let a0 = -PI / 2.0 + 2.0 * PI * i as f64 / p as f64;
        let a1 = -PI / 2.0 + 2.0 * PI * (i + 1) as f64 / p as f64;
        let (x0, y0) = (cx + r * a0.cos(), cy + r * a0.sin());
        let (x1, y1) = (cx + r * a1.cos(), cy + r * a1.sin());
        let grande = if (a1 - a0) > PI { 1 } else { 0 };
        let fill = if i < sombreadas { "url(#trama)" } else { PAPEL };
        let _ = write!(
            s,
            r#"<path d="M{cx},{cy} L{x0:.2},{y0:.2} A{r},{r} 0 {grande} 1 {x1:.2},{y1:.2} Z" fill="{fill}" stroke="{TINTA}" stroke-width="2"/>"#
        );
    }
    s
}

pub fn svg(g: &Grafico) -> String {
    let n = CONTADOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = format!("trama-{n}");
    svg_base(g)
        .replace("id=\"trama\"", &format!("id=\"{id}\""))
        .replace("url(#trama)", &format!("url(#{id})"))
}

fn svg_base(g: &Grafico) -> String {
    match g {
        Grafico::Barra {
            partes,
            sombreadas,
            etiqueta,
        } => {
            let mut c = barra(10.0, 10.0, 380.0, 56.0, *partes, *sombreadas);
            let mut h = 76.0;
            if let Some(e) = etiqueta {
                let _ = write!(
                    c,
                    r#"<text x="200" y="96" text-anchor="middle" font-size="18" fill="{TINTA}">{}</text>"#,
                    esc(e)
                );
                h = 106.0;
            }
            envolver(
                400.0,
                h,
                &c,
                &format!("Barra en {partes} partes, {sombreadas} pintadas"),
            )
        }
        Grafico::Barras { filas } => {
            let mut c = String::new();
            let con_etq = filas.iter().any(|f| f.etiqueta.is_some());
            // El margen izquierdo crece con la etiqueta más larga (≈10 px por carácter a 18 px).
            let largo = filas
                .iter()
                .filter_map(|f| f.etiqueta.as_ref().map(|e| e.chars().count()))
                .max()
                .unwrap_or(0) as f64;
            let x0 = if con_etq {
                (largo * 10.0 + 20.0).clamp(90.0, 200.0)
            } else {
                10.0
            };
            let w = 390.0 - x0;
            for (i, f) in filas.iter().enumerate() {
                let y = 10.0 + i as f64 * 62.0;
                c.push_str(&barra(x0, y, w, 46.0, f.partes, f.sombreadas));
                if let Some(e) = &f.etiqueta {
                    let _ = write!(
                        c,
                        r#"<text x="{}" y="{}" text-anchor="end" font-size="18" fill="{TINTA}">{}</text>"#,
                        x0 - 10.0,
                        y + 30.0,
                        esc(e)
                    );
                }
            }
            envolver(
                400.0,
                10.0 + filas.len() as f64 * 62.0,
                &c,
                "Barras de fracciones",
            )
        }
        Grafico::Circulo { partes, sombreadas } => envolver(
            220.0,
            220.0,
            &circulo(110.0, 110.0, 100.0, *partes, *sombreadas),
            &format!("Círculo en {partes} partes, {sombreadas} pintadas"),
        ),
        Grafico::Circulos { partes, sombreadas } => {
            let p = (*partes).max(1);
            let n = (sombreadas.div_ceil(p)).max(1);
            let mut c = String::new();
            for k in 0..n {
                let s = (*sombreadas).saturating_sub(k * p).min(p);
                c.push_str(&circulo(80.0 + 160.0 * k as f64, 80.0, 70.0, p, s));
            }
            envolver(
                160.0 * n as f64,
                160.0,
                &c,
                &format!("{n} círculos en {p} partes, {sombreadas} pintadas"),
            )
        }
        Grafico::Recta {
            desde,
            hasta,
            divisiones,
            marca,
            pregunta,
        } => recta(*desde, *hasta, *divisiones, marca.as_deref(), *pregunta),
        Grafico::Conjunto {
            total,
            marcados,
            icono,
        } => {
            let por_fila = 6u32.min((*total).max(1));
            let filas = total.div_ceil(por_fila);
            let mut c = String::new();
            for i in 0..*total {
                let (fx, fy) = ((i % por_fila) as f64, (i / por_fila) as f64);
                let (cx, cy) = (40.0 + fx * 62.0, 38.0 + fy * 62.0);
                let marcado = i < *marcados;
                match icono.as_deref() {
                    Some(t) if !t.is_empty() => {
                        let _ = write!(
                            c,
                            r#"<rect x="{}" y="{}" width="54" height="54" rx="10" fill="{}" stroke="{TINTA}" stroke-width="2"/><text x="{cx}" y="{}" text-anchor="middle" font-size="30">{}</text>"#,
                            cx - 27.0,
                            cy - 27.0,
                            if marcado { "#cfcfcf" } else { PAPEL },
                            cy + 11.0,
                            esc(t)
                        );
                        if marcado {
                            let _ = write!(
                                c,
                                r#"<rect x="{}" y="{}" width="54" height="54" rx="10" fill="none" stroke="{TINTA}" stroke-width="5"/>"#,
                                cx - 27.0,
                                cy - 27.0
                            );
                        }
                    }
                    _ => {
                        let fill = if marcado { "url(#trama)" } else { PAPEL };
                        let _ = write!(
                            c,
                            r#"<circle cx="{cx}" cy="{cy}" r="24" fill="{fill}" stroke="{TINTA}" stroke-width="2"/>"#
                        );
                    }
                }
            }
            envolver(
                20.0 + por_fila as f64 * 62.0,
                14.0 + filas as f64 * 62.0,
                &c,
                &format!("Conjunto de {total}, {marcados} marcados"),
            )
        }
    }
}

fn recta(desde: i64, hasta: i64, divisiones: u32, marca: Option<&str>, pregunta: bool) -> String {
    let (x0, x1, y) = (30.0, 470.0, 50.0);
    let unidades = (hasta - desde).max(1) as f64;
    let total = (unidades as u32) * divisiones.max(1);
    let paso = (x1 - x0) / total as f64;
    let mut c = format!(
        r#"<line x1="{}" y1="{y}" x2="{}" y2="{y}" stroke="{TINTA}" stroke-width="3"/>"#,
        x0 - 10.0,
        x1 + 10.0
    );
    for i in 0..=total {
        let x = x0 + paso * i as f64;
        let entero = i % divisiones.max(1) == 0;
        let alto = if entero { 16.0 } else { 9.0 };
        let _ = write!(
            c,
            r#"<line x1="{x:.2}" y1="{}" x2="{x:.2}" y2="{}" stroke="{TINTA}" stroke-width="{}"/>"#,
            y - alto,
            y + alto,
            if entero { 3 } else { 2 }
        );
        if entero {
            let _ = write!(
                c,
                r#"<text x="{x:.2}" y="{}" text-anchor="middle" font-size="20" fill="{TINTA}">{}</text>"#,
                y + 40.0,
                desde + (i / divisiones.max(1)) as i64
            );
        }
    }
    if let Some(m) = marca.and_then(|m| m.parse::<Fraccion>().ok()) {
        let x = x0 + (m.a_f64() - desde as f64) / unidades * (x1 - x0);
        let _ = write!(c, r#"<circle cx="{x:.2}" cy="{y}" r="9" fill="{TINTA}"/>"#);
        let etq = if pregunta {
            "?".to_string()
        } else {
            m.to_string()
        };
        let _ = write!(
            c,
            r#"<text x="{x:.2}" y="{}" text-anchor="middle" font-size="20" font-weight="bold" fill="{TINTA}">{}</text>"#,
            y - 22.0,
            etq
        );
    }
    envolver(500.0, 100.0, &c, "Recta numérica")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Problemas de forma en un gráfico (para la validación del banco).
pub fn problemas(g: &Grafico) -> Vec<String> {
    let mut v = vec![];
    match g {
        Grafico::Barra {
            partes, sombreadas, ..
        }
        | Grafico::Circulo { partes, sombreadas } => {
            if *partes == 0 || *partes > 24 {
                v.push(format!("partes fuera de rango: {partes}"));
            }
            if sombreadas > partes {
                v.push(format!("{sombreadas} sombreadas de {partes} partes"));
            }
        }
        Grafico::Barras { filas } => {
            if filas.is_empty() || filas.len() > 5 {
                v.push("barras: entre 1 y 5 filas".into());
            }
            for f in filas {
                if f.partes == 0 || f.sombreadas > f.partes || f.partes > 24 {
                    v.push(format!("fila inválida {}/{}", f.sombreadas, f.partes));
                }
            }
        }
        Grafico::Circulos { partes, sombreadas } => {
            if *partes == 0 || sombreadas.div_ceil(*partes) > 4 {
                v.push("círculos: máximo 4 círculos".into());
            }
        }
        Grafico::Recta {
            desde,
            hasta,
            divisiones,
            marca,
            ..
        } => {
            if hasta <= desde || *divisiones == 0 || (hasta - desde) as u32 * divisiones > 40 {
                v.push("recta: rango o divisiones inválidos".into());
            }
            if let Some(m) = marca {
                match m.parse::<Fraccion>() {
                    Ok(f) => {
                        if f < Fraccion::entero(*desde) || f > Fraccion::entero(*hasta) {
                            v.push(format!("recta: marca {m} fuera de rango"));
                        }
                    }
                    Err(e) => v.push(format!("recta: marca '{m}' no es fracción: {e}")),
                }
            }
        }
        Grafico::Conjunto {
            total, marcados, ..
        } => {
            if *total == 0 || *total > 30 || marcados > total {
                v.push(format!("conjunto inválido {marcados}/{total}"));
            }
        }
    }
    v
}
