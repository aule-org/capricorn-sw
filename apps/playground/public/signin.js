// Signing in with Google, choosing the student a session is for and saying
// what the session is for. The page never holds the session itself: the server
// keeps it in a cookie it alone reads.

import { $, SUBJECTS, call, chip, describe, openEditor, say } from "./students.js";

const GOOGLE_SCRIPT = "https://accounts.google.com/gsi/client";

// What a family can say a session is for. The first leaves the choice to Aule.
const KINDS = {
  "": "Seguir donde quedamos",
  homework: "Hacer una tarea",
  exam_prep: "Preparar una evaluación",
  exam_review: "Revisar una evaluación",
  explore: "Aprender algo nuevo",
};
// A day matters for these: when the homework is due, when the evaluation is.
const DATED = ["homework", "exam_prep"];
// The one subject Aule has its own exercises for today.
const OWN_SUBJECT = "matematica";

// What the next session is asked with; null on the team's access-code path.
let request = null;
let students = [];
// The subjects the platform keeps for a student; null when it keeps none, and
// then neither they nor what a session is for are offered.
let subjects = null;
let enter = () => {};

/** What the page does once a signed-in person has chosen a student. */
export function onEnter(callback) {
  enter = callback;
}

/** The body of the request for a session: the student's, or the team's code. */
export function sessionRequest(team) {
  return request ?? team;
}

const chosen = (name) => document.querySelector(`input[name="${name}"]:checked`)?.value ?? "";

/** What the family said the session is for, or null when they left it to Aule. */
function objective() {
  const kind = chosen("kind");
  if ($("today").hidden || !kind) return null;
  return {
    kind,
    subject: $("subject").value,
    topic: $("topic").value.trim(),
    due: DATED.includes(kind) && $("due").value ? $("due").value : null,
  };
}

function showToday() {
  const kind = chosen("kind");
  $("today-details").hidden = !kind;
  $("due-line").hidden = !DATED.includes(kind);
  $("subject-note").hidden = $("subject").value === OWN_SUBJECT;
}

// The subject a session starts on: the first one the family wants reinforced.
function showSubject() {
  const student = students.find((one) => one.id === chosen("student"));
  const wanted = student?.subjects?.find((subject) => subjects?.includes(subject));
  $("subject").value = wanted ?? (subjects?.includes(OWN_SUBJECT) ? OWN_SUBJECT : (subjects?.[0] ?? "other"));
  showToday();
}

function edit(student) {
  openEditor(student, subjects, async (saved) => {
    try {
      // Saved or removed: the list is read again. Going back changes nothing.
      if (saved !== undefined) return show(await call("api/account"), saved);
    } catch {
      say("No se pudo leer tu cuenta. Carga la página otra vez.");
    }
    $("who").hidden = false;
    $("account").hidden = false;
  });
}

// Each student is a row to press, with a way into their settings.
function showStudents(preferred) {
  const before = preferred ?? chosen("student");
  const list = $("student-list");
  list.replaceChildren();
  for (const student of students) {
    const row = document.createElement("div");
    row.className = "student";
    const label = document.createElement("label");
    const input = document.createElement("input");
    input.type = "radio";
    input.name = "student";
    input.value = student.id;
    input.addEventListener("change", showSubject);
    const face = document.createElement("span");
    const name = document.createElement("b");
    name.textContent = student.name;
    const about = document.createElement("small");
    about.textContent = describe(student) || "Sin configurar todavía";
    face.append(name, about);
    label.append(input, face);
    const configure = document.createElement("button");
    configure.type = "button";
    configure.className = "quiet";
    configure.textContent = "Configurar";
    configure.setAttribute("aria-label", `Configurar a ${student.name}`);
    configure.addEventListener("click", () => edit(student));
    row.append(label, configure);
    list.append(row);
  }
  const inputs = [...list.querySelectorAll("input")];
  const first = inputs.find((input) => input.value === before) ?? inputs[0];
  if (first) first.checked = true;
  $("no-students").hidden = students.length > 0;
  $("add-student").classList.toggle("quiet", students.length > 0);
  $("enter").hidden = students.length === 0;
  $("today").hidden = students.length === 0 || !subjects;
  showSubject();
}

function show(account, preferred) {
  const signedIn = account.signed_in === true;
  $("signed-out").hidden = signedIn || !account.google_client_id;
  $("who").hidden = !signedIn;
  $("account").hidden = !signedIn;
  $("student-editor").hidden = true;
  // Without Google the access code is the only way in, so it is shown open.
  $("team").open = !account.google_client_id;
  if (!signedIn) return;
  students = account.students ?? [];
  subjects = Array.isArray(account.subjects)
    ? account.subjects.filter((subject) => subject in SUBJECTS)
    : null;
  // A name that already ends in a full stop ("Gonzalo G.") keeps just the one.
  const name = (account.name ?? "").trim().replace(/\.+$/, "");
  $("hello").textContent = name ? `Hola, ${name}.` : "Hola.";
  $("subject").replaceChildren(
    ...(subjects ?? []).map((subject) => new Option(SUBJECTS[subject], subject)),
    new Option("Otra", "other"),
  );
  showStudents(typeof preferred === "string" ? preferred : undefined);
}

function loadGoogle(clientId) {
  const script = document.createElement("script");
  script.src = GOOGLE_SCRIPT;
  script.async = true;
  script.addEventListener("load", () => {
    window.google.accounts.id.initialize({
      client_id: clientId,
      ux_mode: "popup",
      callback: async ({ credential }) => {
        say("");
        try {
          const account = await call("api/signin", { credential });
          show(account);
          // A family with no student yet starts by adding one.
          if (account.signed_in && !account.students?.length) edit(null);
        } catch {
          say("No se pudo entrar con esa cuenta de Google. Inténtalo otra vez.");
        }
      },
    });
    window.google.accounts.id.renderButton($("google"), {
      theme: "outline",
      size: "large",
      text: "signin_with",
      shape: "pill",
      locale: "es",
    });
  });
  // Somebody already signed in has no use for the button, nor for its failure.
  script.addEventListener("error", () => {
    if (!$("signed-out").hidden) say("No se pudo cargar el botón de Google.");
  });
  document.head.append(script);
}

$("kind").replaceChildren(
  ...Object.entries(KINDS).map(([value, text]) => chip("radio", "kind", value, text, value === "")),
);
$("kind").addEventListener("change", showToday);
$("subject").addEventListener("change", showToday);
$("add-student").addEventListener("click", () => edit(null));

$("who").addEventListener("submit", (event) => {
  event.preventDefault();
  say("");
  const student = students.find((one) => one.id === chosen("student"));
  if (!student) return say("Agrega un alumno para empezar.");
  const today = objective();
  request = {
    student_id: student.id,
    language: student.language ?? "es",
    ...(today ? { objective: today } : {}),
  };
  enter();
});

$("sign-out").addEventListener("click", async () => {
  request = null;
  await call("api/signout", {}).catch(() => undefined);
  show(await call("api/account").catch(() => ({ signed_in: false })));
});

$("erase").addEventListener("submit", async (event) => {
  event.preventDefault();
  say("");
  const sure = window.confirm(
    "¿Borrar tu cuenta, tus alumnos y todo lo guardado? No se puede deshacer.",
  );
  if (!sure) return;
  try {
    await call("api/account/erase", { email: $("erase-email").value });
    request = null;
    $("erase-email").value = "";
    show(await call("api/account").catch(() => ({ signed_in: false })));
    say("Tu cuenta y tus datos se borraron.");
  } catch (error) {
    say(
      error?.message === "wrong_email"
        ? "Ese no es el correo con el que entraste."
        : "No se pudo borrar la cuenta. Inténtalo otra vez.",
    );
  }
});

// The team's form goes its own way: no student, the access code instead.
$("entry").addEventListener("submit", () => {
  request = null;
});

call("api/account")
  .then((account) => {
    show(account);
    if (account.google_client_id) loadGoogle(account.google_client_id);
  })
  .catch(() => show({ signed_in: false, google_client_id: null }));
