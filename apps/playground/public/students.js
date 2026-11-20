// A family's students on the entry screen: the words for what can be chosen,
// and the editor where a guardian says who a student is for Aule and what is
// kept about them. Nothing is saved until the guardian presses the button.

export const $ = (id) => document.getElementById(id);

export const GRADES = {
  P1: "1.º de primaria",
  P2: "2.º de primaria",
  P3: "3.º de primaria",
  P4: "4.º de primaria",
  P5: "5.º de primaria",
  P6: "6.º de primaria",
  S1: "1.º de secundaria",
  S2: "2.º de secundaria",
  S3: "3.º de secundaria",
  S4: "4.º de secundaria",
  S5: "5.º de secundaria",
};

export const SUBJECTS = {
  matematica: "Matemática",
  comunicacion: "Comunicación",
  ciencia: "Ciencia y Tecnología",
  personal_social: "Personal Social",
  ingles: "Inglés",
};

const TONES = {
  calido: ["Cálido", "cercano y paciente"],
  jugueton: ["Juguetón", "con humor ligero"],
  aventurero: ["Aventurero", "cada tema es un reto por descubrir"],
  tranquilo: ["Tranquilo", "pausado, sin apuro"],
};

// Things many students like, to pick from; a family can add its own.
const INTERESTS = [
  "fútbol",
  "vóley",
  "baile",
  "música",
  "dibujo",
  "videojuegos",
  "animales",
  "dinosaurios",
  "el espacio",
  "cocina",
  "cuentos",
  "inventos",
];
const MAX_INTERESTS = 10;

export async function call(path, body) {
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const answer = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(answer.error ?? "failed");
  return answer;
}

export function say(text) {
  const message = $("message");
  message.textContent = text;
  message.hidden = !text;
}

/** A choice drawn as a chip: a checkbox or a radio the person presses by its label. */
export function chip(type, name, value, text, checked) {
  const label = document.createElement("label");
  label.className = "chip";
  const input = document.createElement("input");
  input.type = type;
  input.name = name;
  input.value = value;
  input.checked = checked;
  const face = document.createElement("span");
  face.textContent = text;
  label.append(input, face);
  return label;
}

const checked = (name) =>
  [...document.querySelectorAll(`input[name="${name}"]:checked`)].map((input) => input.value);

/** One line about a student for their card: what the family told Aule. */
export function describe(student) {
  const parts = [];
  if (student.grade) parts.push(GRADES[student.grade]);
  if (student.tone && student.tone !== "calido") parts.push(`tono ${TONES[student.tone][0].toLowerCase()}`);
  if (student.interests?.length) parts.push(student.interests.slice(0, 3).join(", "));
  return parts.join(" · ");
}

let editing = null;
let done = () => {};

function count(number, one, many) {
  return `${number} ${number === 1 ? one : many}`;
}

function day(text) {
  const date = new Date(`${text}T12:00:00`);
  return Number.isNaN(date.getTime())
    ? text
    : date.toLocaleDateString("es", { day: "numeric", month: "long" });
}

// What is kept about a student who already has sessions.
async function showKnown(student) {
  const section = $("known");
  section.hidden = true;
  $("memories").replaceChildren();
  $("past").replaceChildren();
  let known;
  try {
    known = await call("api/student/known", { student_id: student.id });
  } catch {
    return;
  }
  // The editor may have moved on to another student while this was on its way.
  if (editing?.id !== student.id) return;
  $("known-title").textContent = `Lo que Aule sabe de ${student.name}`;
  const worked = known.mastered + known.practising;
  let progress = worked
    ? `Domina ${count(known.mastered, "habilidad", "habilidades")} y está practicando ${known.practising}.`
    : "Todavía no hay ejercicios resueltos.";
  if (known.next.length) progress += ` Lo que viene: ${known.next.join(", ")}.`;
  $("progress").textContent = progress;
  for (const memory of known.memories) {
    const item = document.createElement("li");
    const forget = document.createElement("button");
    forget.type = "button";
    forget.className = "quiet";
    forget.textContent = "Olvidar";
    forget.addEventListener("click", async () => {
      forget.disabled = true;
      try {
        await call("api/student/forget", { student_id: student.id, memory_id: memory.id });
        item.remove();
      } catch {
        forget.disabled = false;
        say("No se pudo olvidar. Inténtalo otra vez.");
      }
    });
    const text = document.createElement("span");
    text.textContent = memory.text;
    item.append(text, forget);
    $("memories").append(item);
  }
  if (!known.memories.length) {
    const none = document.createElement("li");
    none.className = "none";
    none.textContent = `Aule todavía no recuerda nada de ${student.name}.`;
    $("memories").append(none);
  }
  for (const session of known.sessions) {
    const item = document.createElement("li");
    const when = document.createElement("b");
    when.textContent = day(session.day);
    const text = document.createElement("span");
    text.textContent = session.next ? `${session.summary} ${session.next}` : session.summary;
    item.append(when, text);
    $("past").append(item);
  }
  $("past-title").hidden = known.sessions.length === 0;
  section.hidden = false;
}

function fill(student, subjects) {
  $("editor-title").textContent = student ? `Configurar a ${student.name}` : "Nuevo alumno";
  $("student-name").value = student?.name ?? "";
  $("student-grade").replaceChildren(
    new Option("Prefiero no decirlo", ""),
    ...Object.entries(GRADES).map(([value, text]) => new Option(text, value)),
  );
  $("student-grade").value = student?.grade ?? "";
  $("subjects-field").hidden = !subjects;
  $("student-subjects").replaceChildren(
    ...(subjects ?? []).map((value) =>
      chip("checkbox", "subjects", value, SUBJECTS[value], student?.subjects?.includes(value) ?? false),
    ),
  );
  // The student's own come first and keep their order: the first ones are
  // what the tutor reaches for.
  const liked = student?.interests ?? [];
  $("student-interests").replaceChildren(
    ...[...new Set([...liked, ...INTERESTS])].map((interest) =>
      chip("checkbox", "interests", interest, interest, liked.includes(interest)),
    ),
  );
  $("interest-new").value = "";
  $("student-tone").replaceChildren(
    ...Object.entries(TONES).map(([value, [title, text]]) => {
      const label = chip("radio", "tone", value, "", (student?.tone ?? "calido") === value);
      label.className = "tone";
      const strong = document.createElement("b");
      strong.textContent = title;
      label.lastChild.append(strong, ` ${text}`);
      return label;
    }),
  );
  $("student-note").value = student?.note ?? "";
  $("student-language").value = student?.language ?? "es";
  $("student-remembers").checked = student?.remembers ?? true;
  $("student-transcripts").checked = student?.keepsTranscripts ?? false;
  $("consent").checked = false;
  $("consent-line").hidden = Boolean(student);
  $("remove-student").hidden = !student;
  $("known").hidden = true;
}

/**
 * Opens the editor for a student, or for a new one. `subjects` are the ones
 * that can be kept for a student, null when none can. `finished` is told the
 * id of the student that was saved, null when one was removed, and undefined
 * when the guardian went back without changing anything.
 */
export function openEditor(student, subjects, finished) {
  editing = student;
  done = finished;
  say("");
  fill(student, subjects);
  $("who").hidden = true;
  $("account").hidden = true;
  $("student-editor").hidden = false;
  $("student-name").focus();
  if (student) showKnown(student);
}

function close(result) {
  $("student-editor").hidden = true;
  editing = null;
  done(result);
}

$("interest-add").addEventListener("click", () => {
  const interest = $("interest-new").value.trim().toLowerCase();
  if (!interest) return;
  const there = [...document.querySelectorAll('input[name="interests"]')].find(
    (input) => input.value.toLowerCase() === interest,
  );
  if (there) there.checked = true;
  else $("student-interests").append(chip("checkbox", "interests", interest, interest, true));
  $("interest-new").value = "";
});

// Enter in the box adds the interest; it does not save the whole form.
$("interest-new").addEventListener("keydown", (event) => {
  if (event.key !== "Enter") return;
  event.preventDefault();
  $("interest-add").click();
});

$("student-editor").addEventListener("submit", async (event) => {
  event.preventDefault();
  say("");
  const interests = checked("interests");
  if (interests.length > MAX_INTERESTS) return say("Elige hasta diez cosas que le gusten.");
  if (!editing && !$("consent").checked) return say("Para crear al alumno hace falta tu aceptación.");
  const profile = {
    name: $("student-name").value.trim(),
    grade: $("student-grade").value || null,
    language: $("student-language").value,
    interests,
    tone: checked("tone")[0] ?? "calido",
    note: $("student-note").value.trim(),
    ...($("subjects-field").hidden ? {} : { subjects: checked("subjects") }),
    remembers: $("student-remembers").checked,
    keeps_transcripts: $("student-transcripts").checked,
  };
  if (!profile.name) return say("Escribe cómo quieres que llamemos al alumno.");
  $("save-student").disabled = true;
  try {
    const saved = editing
      ? await call("api/student/save", { student_id: editing.id, ...profile })
      : await call("api/student", { consent: true, ...profile });
    close(saved.id);
  } catch {
    say("No se pudo guardar. Inténtalo otra vez.");
  } finally {
    $("save-student").disabled = false;
  }
});

$("cancel-student").addEventListener("click", () => close(undefined));

$("remove-student").addEventListener("click", async () => {
  const student = editing;
  if (!student) return;
  const sure = window.confirm(
    `¿Quitar a ${student.name}? Se borra lo que Aule recuerda de este alumno. No se puede deshacer.`,
  );
  if (!sure) return;
  try {
    await call("api/student/remove", { student_id: student.id });
    close(null);
  } catch {
    say("No se pudo quitar al alumno. Inténtalo otra vez.");
  }
});
