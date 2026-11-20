# Curriculum

Skill catalogs for the platform's learner model: what a child can be taught, in units small enough to practise in one voice session and to be judged right or wrong on, and which skills must be mastered before another is offered.

```text
math-primaria.json   mathematics for Peruvian primary school, grades 1 to 6 (188 skills)
test/                checks every *.json catalog in this folder
```

## Status: an unreviewed draft

`math-primaria.json` is a **draft written from general knowledge of the structure of Peru's Currículo Nacional de la Educación Básica (CNEB)**, organised by the four competencies of its Matemática area and by what is broadly expected in each cycle (III = grades 1-2, IV = grades 3-4, V = grades 5-6).

- It was **not checked against the Programa Curricular de Educación Primaria**. It quotes no official text and carries no official codes.
- It has **not been reviewed by a teacher**.
- The prerequisite links are a reasoned guess at what a child needs first, not a validated learning progression.

It is good enough to give the tutor something sensible to practise next. It **needs that check and that review before it is used to tell a parent that a child is at, above or below grade level**: the `grade` of a skill here is where this draft placed it, not an official statement of when it is expected.

### Placements we were unsure about

Where it was unclear in which grade the CNEB expects a topic, the skill sits where Peruvian textbooks commonly teach it. A reviewer should look at these first:

| Skill | Placed in | Doubt |
| --- | --- | --- |
| `numeros.contar-hasta-100` | P1 | The CNEB may only expect numbers up to 20 in grade 1; textbooks reach 99 by the end of the year. Sums and subtractions in P1 stay within 20. |
| `numeros.ordinales-hasta-decimo`, `numeros.ordinales-hasta-vigesimo` | P1, P2 | The range of ordinals expected in each grade is from memory. |
| `azar.siempre-a-veces-nunca`, `azar.posible-e-imposible`, `azar.seguro-posible-imposible` | P1, P2, P3 | The order in which this vocabulary is introduced is from memory. |
| `multiplicacion.sumas-repetidas` | P2 | Multiplication may only be expected from grade 3; textbooks introduce it as repeated addition in grade 2. |
| `longitud.metro-y-centimetro` | P2 | Grade 2 may still expect non-conventional units only. |
| `fracciones.mitad-tercio-cuarto` | P3 | Whether thirds belong in grade 3 or only halves and quarters. |
| `datos.moda` | P4 | Could belong in grade 3 or 5. |
| `geometria.angulos-recto-agudo-obtuso`, `geometria.angulos-en-grados` | P4, P5 | Angle classification and measuring in degrees could each sit a grade later. |
| `numeros.multiplos-y-divisores` | P5 | Could belong in grade 6. |
| `operaciones.combinadas-con-parentesis` | P5 | A textbook topic; unsure the CNEB names order of operations for this grade. |
| `datos.promedio` | P5 | The arithmetic mean could belong in grade 6. |
| `geometria.area-triangulo`, `geometria.volumen-contando-cubos` | P5 | Could belong in grade 6. |
| `decimales.multiplicar-por-natural`, `decimales.multiplicar-decimales`, `decimales.dividir-entre-natural` | P6 | Unsure how far decimal multiplication and division are expected in primary. |
| `fracciones.multiplicar`, `fracciones.dividir` | P6 | Division with fractions in particular may be beyond primary. |
| `numeros.mcm-y-mcd`, `numeros.primos-y-compuestos` | P6 | Common in grade 6 textbooks; unsure the CNEB expects them in primary. |
| `numeros.potencias-cuadrado-y-cubo` | P6 | From memory. |
| `azar.probabilidad-como-fraccion`, `azar.comparar-probabilidades` | P6 | Probability as a fraction may be a secondary-school expectation. |
| `ecuaciones.dos-pasos`, `patrones.expresar-regla-con-letras` | P6 | May go further into algebra than primary expects. |

Known gaps: symmetry, nets of solids, the length of a circumference, inequalities and inverse proportionality are left out, some because they are hard to practise by voice and some because their place in primary was unclear.

## Shape of a skill

```json
{
  "id": "fracciones.sumar-igual-denominador",
  "subject": "math",
  "name": "Sumar fracciones de igual denominador",
  "grade": "P4",
  "description": "Suma fracciones con el mismo denominador … Ejemplo: «¿Cuánto es dos octavos más tres octavos?» (cinco octavos).",
  "prerequisites": ["fracciones.comparar-igual-denominador"]
}
```

- **`grade`**: `P1` to `P6`, grades 1 to 6 of primaria. A child with no history is offered skills without prerequisites, their own grade first.
- **`name`** (at most 160 characters) and **`description`** (at most 600) are in Spanish as used in Peru and are read by the model that plans sessions; the name may also be shown to parents. The description says what the child can do and gives an exercise that works out loud, with the expected answer in parentheses. No skill needs the tutor to show a figure or the child to draw one; the child may work in a notebook and hold it up to the camera.
- **`prerequisites`**: ids that must be mastered before this skill is offered as new. They never point to a later grade and form no cycles. Only P1 (and one P2 skill) can be started with no history, so a child who starts in a later grade is first offered those until evidence of earlier skills is recorded.

### Ids

`<strand>.<skill>`, lowercase Spanish without accents or `ñ`, words joined with hyphens: `numeros.contar-hasta-20`, `geometria.perimetro-rectangulo`. An id is the key a child's history is stored under, so **do not rename an id once children have practised it**; add a new skill instead.

The strand says which of the four competencies the skill was written under:

| Competency | Strands | Skills |
| --- | --- | --- |
| Resuelve problemas de cantidad | `numeros`, `suma-resta`, `multiplicacion`, `division`, `operaciones`, `fracciones`, `decimales`, `porcentajes`, `dinero`, `tiempo`, `masa` | 104 |
| Resuelve problemas de regularidad, equivalencia y cambio | `patrones`, `igualdades`, `cambio`, `ecuaciones`, `proporcionalidad` | 22 |
| Resuelve problemas de forma, movimiento y localización | `geometria`, `ubicacion`, `longitud`, `capacidad` | 41 |
| Resuelve problemas de gestión de datos e incertidumbre | `datos`, `azar` | 21 |

Skills per grade: P1 28, P2 28, P3 32, P4 34, P5 33, P6 33.

## Load it

From the repository root, with the platform running (see `apps/platform/README.md`):

```sh
curl -X PUT "$AULE_PLATFORM_URL/v1/skills" \
  -H "Authorization: Bearer $AULE_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  --data @curriculum/math-primaria.json
```

The request inserts new skills and updates existing ones by id (at most 2000 per request). It does not delete: a skill removed from the file stays in the database.

## Check

```sh
pnpm --filter @aule/curriculum test
```

The tests check, for every `*.json` catalog here: the `{"skills": [...]}` shape; unique ids matching the platform's slug rule; required fields and length limits; `subject` is `math` and `grade` is `P1`..`P6`; every prerequisite exists, is not the skill itself and is not from a later grade; the graph has no cycles; every grade has at least 15 skills and P1 at least 5 with no prerequisites.
