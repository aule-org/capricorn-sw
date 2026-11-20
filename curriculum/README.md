# Curriculum

Skill catalogs for the platform's learner model: what a child can be taught, in units small enough to practise in one voice session and to be judged right or wrong on, and which skills must be mastered before another is offered.

```text
math-primaria.json             mathematics for Peruvian primary school (188 skills: 181 in grades 1 to 6, 7 that the official program puts in secondary)
math-secundaria-algebra.json   algebra for the first three years of secondary school (17 skills)
test/                checks every *.json catalog in this folder
```

## Status: checked against MINEDU's programs, not reviewed by a teacher

Both catalogs were first written from general knowledge of Peru's Currículo Nacional de la Educación Básica (CNEB). On 2026-11-20 every skill in them (205) was checked against the Matemática area of MINEDU's two programas curriculares, and its `grade` and prerequisites were corrected where the documents are clear:

- ***Programa curricular de Educación Primaria***, Educación Básica Regular, MINEDU, 2016 (PDF of March 2017, 396 pages, linked from minedu.gob.pe/curriculo). Area 6.7 Matemática: the four competencies, their capacities, the level expected at the end of each cycle, and the desempeños per grade. Pages used: cantidad 236–241, regularidad 246–251, forma 256–261, datos 266–270. These are the printed page numbers; in the PDF each is two pages later.
- ***Programa curricular de Educación Secundaria***, MINEDU, 2016 (approved with R.M. 649-2016-MINEDU; a scanned PDF of 259 pages, same site). Area 6.8 Matemática, pp. 147–176. Pages used: cantidad 151–153, regularidad 158–161, forma 165, datos 172.

**No teacher has reviewed either catalog.** The check is one reading of the official documents, not a validated learning progression, and the prerequisite links are still a reasoned guess at what a child needs first. The catalogs are good enough to choose what a child practises next; they need a teacher's review before they are used to tell a parent that a child is at, above or below grade level. The catalogs quote no official text beyond short phrases here and carry no official codes.

### How a grade was decided

A skill's `grade` is the first grade whose desempeños ask for everything its description and examples ask for. When a skill mixes something of one grade with something of a later one, it goes to the later grade. Topics the primary program does not reach moved to the first secondary year that names them; they keep their ids and stay in `math-primaria.json`. Ids, names and descriptions are unchanged.

### Changes made

Pages are the printed ones of the program for that grade (primary for P, secondary for S).

| Skill | Grade | Why |
| --- | --- | --- |
| `numeros.contar-hasta-100` | P1 → P2 | Grade 1 works with numbers to 20 and counts collections to 50; two-digit numbers are grade 2 (p. 237). |
| `datos.leer-pictograma` | P2 → P1 | Pictograms where each symbol is one unit are a grade 1 desempeño (p. 266). |
| `datos.leer-grafico-de-barras-simple` | P2 → P1 | Simple bar graphs without a scale are a grade 1 desempeño (p. 266); see the prerequisite change below. |
| `ubicacion.cuadricula-filas-y-columnas` | P3 → P2 | Positions and moves on grids are grades 1 and 2 (pp. 256–257); grade 3 does not mention grids (pp. 258–259). |
| `multiplicacion.sumas-repetidas` | P2 → P3 | Grade 2 only adds, subtracts, doubles and halves (p. 237); repeated addition ("reiterar") and multiplication start in grade 3 (pp. 238–239). |
| `longitud.metro-y-centimetro` | P2 → P3 | Grade 2 measures with hands, steps and feet (p. 257); centimetre and metre are grade 3 (p. 259). |
| `capacidad.comparar-recipientes` | P2 → P3 | Capacity ("contiene más que") first appears in grade 3 (p. 259). |
| `tiempo.leer-horas-en-punto-y-media` | P2 → P4 | Grade 2 measures time in days and weekly timetables (p. 237); exact hours are grade 3 and the half hour grade 4 (p. 239). |
| `tiempo.leer-horas-y-minutos` | P3 → P4 | Quarter hours are grade 4 (p. 239); it also builds on the skill above. |
| `fracciones.mitad-tercio-cuarto` | P3 → P4 | Grade 3 has no fractions; the fraction as part of a whole, with denominators 2, 3, 4, 5, 6, 8 and 10, is grade 4 (pp. 238–239). |
| `capacidad.litros` | P3 → P4 | Litres "con fracciones" are grade 4 (p. 259). |
| `masa.kilogramos-y-gramos` | P3 → P4 | Grade 3 uses the kilogram only; the gram is grade 4 (p. 239). |
| `longitud.convertir-metros-y-centimetros` | P3 → P4 | Converting units of length is grade 4 (p. 259). |
| `geometria.prismas-caras-aristas-vertices` | P3 → P4 | Faces, vertices and edges of cubes and prisms are grade 4 (p. 258). |
| `datos.grafico-de-barras-con-escala` | P4 → P3 | Bar graphs with a scale of 2, 5 or 10 are grade 3 (p. 268). |
| `fracciones.fraccion-de-una-cantidad` | P4 → P5 | Grade 4 has the fraction as part of a whole; the fraction of a quantity and as an operator is grade 5 (pp. 239, 241). |
| `masa.convertir-kilogramos-y-gramos` | P4 → P5 | Converting units of mass is grade 5 (p. 241). |
| `longitud.kilometros-y-metros` | P4 → P5 | Grade 4 measures in metres and centimetres; the kilometre is grade 5 (pp. 259, 261). |
| `numeros.multiplos-y-divisores` | P5 → P6 | Grade 5 names multiples; divisors, with primes, are grade 6 (p. 241). |
| `geometria.clasificar-triangulos` | P5 → P6 | Grade 5 works with quadrilaterals; triangles and circles are grade 6 (p. 260). |
| `geometria.area-triangulo` | P5 → P6 | The area of triangles is grade 6 (pp. 260–261). |
| `geometria.volumen-contando-cubos` | P5 → P6 | Grade 5 measures capacity; volume is grade 6 (pp. 260–261). |
| `numeros.millones` | P6 → S1 | Primary stops at six digits (pp. 240–241); place value to the millions is secondary year 1 (p. 151). |
| `decimales.milesimos` | P6 → S1 | Primary stops at hundredths (pp. 240–241); year 1 works with decimals without that limit (p. 151). |
| `fracciones.dividir` | P6 → S1 | Primary adds, subtracts and multiplies fractions (p. 240); dividing them is year 1 (p. 151). |
| `porcentajes.calcular-porcentaje-de-una-cantidad` | P6 → S1 | Primary uses only 10, 25, 50, 75 and 100 % (pp. 240–241); other percentages are year 1 (pp. 151–152). |
| `porcentajes.descuentos-y-aumentos` | P6 → S1 | Percentage increases and discounts are year 1 (p. 151). |
| `datos.grafico-circular-con-porcentajes` | P6 → S1 | Pie charts are not in primary (pp. 266–270); they are year 1 (p. 172). |
| `geometria.prismas-y-piramides` | P5 → S2 | Primary and year 1 name prisms but not pyramids; pyramids are year 2 (p. 165). |
| `algebra.ecuaciones-con-fracciones` | S1 → S2 | Year 1 solves linear equations with integer coefficients, year 2 with rational ones (p. 158). |
| `algebra.sistemas-2x2` | S2 → S3 | Years 1 and 2 have no systems; systems of two linear equations are year 3 (pp. 158, 160). |
| `algebra.plantear-sistemas` | S2 → S3 | As above. |

Prerequisites changed so that none comes from a later grade:

- `fracciones.equivalentes-y-simplificar` (P5) no longer requires `numeros.multiplos-y-divisores` (now P6). Grades 4 and 5 already simplify fractions as a calculation strategy (pp. 239, 241).
- `datos.leer-grafico-de-barras-simple` (now P1) requires `numeros.comparar-hasta-20` instead of `numeros.comparar-y-ordenar-hasta-100`; its examples stay within 20.
- `geometria.volumen-contando-cubos` (now P6) requires `geometria.prismas-caras-aristas-vertices` instead of `geometria.prismas-y-piramides` (now S2): the cubes it counts fill a box.

Checked and left where they were, among the placements the first draft doubted: the ordinals (to the tenth in P1, the twentieth in P2, p. 237); "siempre, a veces, nunca" in P1, "posible e imposible" in P2 and "seguro, posible, imposible" in P3 (pp. 266, 268); `datos.moda` in P4 (p. 268); `datos.promedio` in P5 (pp. 268, 270); classifying angles against the right angle in P4 and measuring them in P5 (pp. 259, 261); multiplying decimals and fractions, primes and composites, squares and cubes in P6 (pp. 240–241); probability as a fraction in P6 (p. 270); two-step equations and a pattern's rule with a letter in P6 (p. 251).

### Doubts that remain

- **Quadratic equations belong in S4.** Year 3 has only equations of the form ax² = c; the complete ax² + bx + c = 0 is year 4 (p. 160). `algebra.cuadraticas-por-factorizacion` and `algebra.formula-general` should be S4, and so should the ax² + bx = 0 half of `algebra.cuadraticas-incompletas` and the general trinomials of `algebra.factorizar-trinomios`. They stay in S3 because the tests require at least four skills in every grade a non-primary catalog uses: only two are clearly S4, and 17 skills with six in S1 cannot fill S2, S3 and S4 with four each. Adding S4 skills, or changing that minimum, is the owner's decision.
- **Notable products are named first in year 3** (p. 161), where they are used to simplify and solve; year 2 simplifies with the properties of equality and of the operations (p. 159). `algebra.productos-notables` and `algebra.diferencia-de-cuadrados`, which builds on it, stay in S2 for the same reason: moving them leaves S2 with three skills.
- **Factoring is not named in years 1 to 4.** `algebra.factor-comun`, `algebra.diferencia-de-cuadrados` and `algebra.factorizar-trinomios` sit where textbooks put them.
- `decimales.decimos-y-centesimos` (P5): grade 5 names decimals to the tenth and grade 6 to the hundredth (p. 241), but grade 5 already adds and subtracts decimals and the level for the end of grades 5–6 reaches the hundredth (p. 240). Moving it would push decimal addition, which grade 5 names, to P6.
- `tiempo.leer-horas-y-minutos` and `tiempo.duracion-y-horarios` (P4) work in minutes, which the program names in grade 6 (p. 241); grade 5 converts units of time. There is no skill for reading exact hours only, which is grade 3 (p. 239).
- `capacidad.litros-y-mililitros` (P4): millilitres are not named; grade 4 uses litres with fractions, grade 5 with decimals (pp. 259, 261).
- `ubicacion.izquierda-y-derecha` (P1): the grade 1 list of position words leaves out left and right, and grade 2 has "a la derecha" (p. 257). Both lists are examples, so it stays.
- `numeros.redondear-a-decena-y-centena` (P3): grade 3 makes approximations; rounding to multiples of 10 is named in grade 4 (p. 239).
- `operaciones.combinadas-con-parentesis` (P5) and `numeros.mcm-y-mcd` (P6) are not named in primary; year 1 names decomposition into prime factors (p. 151).
- `proporcionalidad.razones` (P6): primary names direct proportionality (p. 251); ratios appear in an example of year 1 and as "la fracción como razón" in year 2 (p. 151).
- `ubicacion.ampliar-y-reducir-figuras` (P6) is already a grade 5 desempeño (p. 260); it stays in P6, which also names it, because it builds on `proporcionalidad.razones`.
- `datos.barras-dobles` (P5): the grade desempeños name double bar graphs in grade 6 (p. 270), the level for the end of grades 3–4 already does (p. 267).
- `azar.resultados-posibles` (P5): grades 3 and 4 already list the possible results of everyday events (p. 268); the skill's dice and coins fit grades 5 and 6.
- `geometria.angulos-en-grados` (P5): measuring angles is grade 5 (p. 261); the sum of a triangle's angles is not named.
- `decimales.dividir-entre-natural` (P6): grade 6 divides two whole numbers with an exact decimal quotient (p. 240); dividing a decimal by a whole number is not named.
- `geometria.perimetro-de-poligonos` (P3): grade 3 measures "el contorno de una figura" (p. 259); the word perimeter appears in grade 4.
- Money (`dinero.*`): the primary program has no desempeño on money; those skills sit where their numbers do.

### What the programs ask for that the catalogs do not have

Listed, not added:

- Symmetry: axes of symmetry and symmetric figures (grades 3–4, pp. 258–259), reflections (grade 5, p. 261).
- Inequalities with > and < (grade 6, p. 251) and patterns of second order such as 13, 15, 18, 22, 27 (grade 5, p. 251).
- How a quantity changes over time, read from a table (grade 3, p. 249).
- Comparing surfaces without units (grades 3–4, p. 259).
- Temperature (grade 6, p. 241); decades and centuries (grade 5, p. 241); reading exact hours (grade 3, p. 239).
- Perimeter and area of the circle, and the cylinder (grade 6, p. 260); nets of solids (grade 4, p. 259; grade 6, p. 261).
- Turns on the Cartesian plane and the cardinal points (grade 6, pp. 260–261).
- Operations with integers (year 1, p. 151): the algebra skills already use negative numbers, and nothing teaches them.
- Arithmetic and geometric progressions, linear inequalities, linear, affine and quadratic functions, and inverse proportionality (years 1–4, pp. 158–161).
- The median (year 1, p. 172).

## Secondary algebra

`math-secundaria-algebra.json` covers the strand of *Resuelve problemas de regularidad, equivalencia y cambio* in the first three years of secundaria: algebraic language, like terms and first-degree equations (S1), equations with fractions, polynomial products and factoring (S2), and systems of two equations and quadratic equations (S3). Its first skills build on primary ones (`ecuaciones.dos-pasos`, `patrones.expresar-regla-con-letras`). It was checked as above; see the doubts about quadratics, notable products and factoring. Every example in it is one the platform can check exactly (see `apps/platform/src/algebra.rs`). Inequalities and functions are left out: their answers are not a number or a polynomial.

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

- **`grade`**: `P1` to `P6`, grades 1 to 6 of primaria; `S1` to `S5`, years 1 to 5 of secundaria. A child with no history is offered skills without prerequisites, their own grade first.
- **`name`** (at most 160 characters) and **`description`** (at most 600) are in Spanish as used in Peru and are read by the model that plans sessions; the name may also be shown to parents. The description says what the child can do and gives an exercise that works out loud, with the expected answer in parentheses. No skill needs the tutor to show a figure or the child to draw one; the child may work in a notebook and hold it up to the camera.
- **`prerequisites`**: ids that must be mastered before this skill is offered as new. They never point to a later grade and form no cycles. Only P1 (and one P3 skill, `capacidad.comparar-recipientes`) can be started with no history, so a child who starts in a later grade is first offered those until evidence of earlier skills is recorded.

### Ids

`<strand>.<skill>`, lowercase Spanish without accents or `ñ`, words joined with hyphens: `numeros.contar-hasta-20`, `geometria.perimetro-rectangulo`. An id is the key a child's history is stored under, so **do not rename an id once children have practised it**; add a new skill instead.

The strand says which of the four competencies the skill was written under:

| Competency | Strands | Skills |
| --- | --- | --- |
| Resuelve problemas de cantidad | `numeros`, `suma-resta`, `multiplicacion`, `division`, `operaciones`, `fracciones`, `decimales`, `porcentajes`, `dinero`, `tiempo`, `masa` | 104 |
| Resuelve problemas de regularidad, equivalencia y cambio | `patrones`, `igualdades`, `cambio`, `ecuaciones`, `proporcionalidad` | 22 |
| Resuelve problemas de forma, movimiento y localización | `geometria`, `ubicacion`, `longitud`, `capacidad` | 41 |
| Resuelve problemas de gestión de datos e incertidumbre | `datos`, `azar` | 21 |

Skills per grade in `math-primaria.json`: P1 29, P2 24, P3 29, P4 37, P5 31, P6 31, and S1 6, S2 1. In `math-secundaria-algebra.json`: S1 6, S2 5, S3 6.

## Load it

The platform's image carries every catalog in this folder and `aule-platform serve` loads them when it starts, so a deploy is enough: new skills are added and changed ones updated. To load one by hand, from the repository root, with the platform running (see `apps/platform/README.md`):

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

The tests check, for every `*.json` catalog here: the `{"skills": [...]}` shape; unique ids matching the platform's slug rule; required fields and length limits; `subject` is `math` and `grade` is `P1`..`P6` or `S1`..`S5`; every prerequisite exists in some catalog here, is not the skill itself and is not from a later grade; the graph has no cycles; a primary catalog has at least 15 skills in every grade and P1 at least 5 with no prerequisites, and any other catalog at least 4 in each grade it uses.
