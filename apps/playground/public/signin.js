// Signing in with Google and choosing the student a session is for. The page
// never holds the session itself: the server keeps it in a cookie it alone reads.

const $ = (id) => document.getElementById(id);
const GOOGLE_SCRIPT = "https://accounts.google.com/gsi/client";

// The student chosen for the next session; null on the team's access-code path.
let studentId = null;
let students = [];
let enter = () => {};

/** What the page does once a signed-in person has chosen a student. */
export function onEnter(callback) {
  enter = callback;
}

/** The body of the request for a session: the student's, or the team's code. */
export function sessionRequest(team) {
  return studentId ? { student_id: studentId, language: $("student-language").value } : team;
}

async function call(path, body) {
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const answer = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(answer.error ?? "failed");
  return answer;
}

function say(text) {
  const message = $("message");
  message.textContent = text;
  message.hidden = !text;
}

function chosen() {
  return document.querySelector('input[name="student"]:checked')?.value ?? "new";
}

function showNewStudent() {
  const creating = chosen() === "new";
  $("new-student").hidden = !creating;
  $("consent-line").hidden = !creating;
}

function show(account) {
  const signedIn = account.signed_in === true;
  $("signed-out").hidden = signedIn || !account.google_client_id;
  $("who").hidden = !signedIn;
  $("account").hidden = !signedIn;
  // Without Google the access code is the only way in, so it is shown open.
  $("team").open = !account.google_client_id;
  if (!signedIn) return;
  students = account.students ?? [];
  // A name that already ends in a full stop ("Gonzalo G.") keeps just the one.
  const name = (account.name ?? "").trim().replace(/\.+$/, "");
  $("hello").textContent = name ? `Hola, ${name}.` : "Hola.";
  const list = $("student-list");
  list.replaceChildren();
  for (const [index, student] of [...students, { id: "new", name: "Otro alumno" }].entries()) {
    const label = document.createElement("label");
    label.className = "check choice";
    const input = document.createElement("input");
    input.type = "radio";
    input.name = "student";
    input.value = student.id;
    input.checked = index === 0;
    input.addEventListener("change", showNewStudent);
    label.append(input, ` ${student.name}`);
    list.append(label);
  }
  $("students").hidden = students.length === 0;
  showNewStudent();
  showKept();
}

// The students kept in the account, each with a way to remove it.
function showKept() {
  const kept = $("kept-students");
  kept.replaceChildren();
  for (const student of students) {
    const row = document.createElement("p");
    row.className = "kept";
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "quiet";
    remove.textContent = "Quitar";
    remove.addEventListener("click", async () => {
      const sure = window.confirm(
        `¿Quitar a ${student.name}? Se borra lo que Aule recuerda de este alumno. No se puede deshacer.`,
      );
      if (!sure) return;
      try {
        await call("api/student/remove", { student_id: student.id });
        if (studentId === student.id) studentId = null;
        show(await call("api/account"));
      } catch {
        say("No se pudo quitar al alumno. Inténtalo otra vez.");
      }
    });
    row.append(`${student.name} `, remove);
    kept.append(row);
  }
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
          show(await call("api/signin", { credential }));
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
  script.addEventListener("error", () => say("No se pudo cargar el botón de Google."));
  document.head.append(script);
}

$("who").addEventListener("submit", async (event) => {
  event.preventDefault();
  say("");
  $("enter").disabled = true;
  try {
    let id = chosen();
    if (id === "new") {
      const name = $("student-name").value.trim();
      if (!name) return say("Escribe el nombre del alumno.");
      if (!$("consent").checked) return say("Para crear al alumno hace falta tu aceptación.");
      const created = await call("api/student", {
        name,
        consent: true,
        language: $("student-language").value,
      });
      id = created.id;
      $("student-name").value = "";
      $("consent").checked = false;
      show(await call("api/account"));
      const added = document.querySelector(`input[name="student"][value="${id}"]`);
      if (added) added.checked = true;
      showNewStudent();
    }
    studentId = id;
    enter();
  } catch {
    say("No se pudo continuar. Inténtalo otra vez.");
  } finally {
    $("enter").disabled = false;
  }
});

$("sign-out").addEventListener("click", async () => {
  studentId = null;
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
    studentId = null;
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
  studentId = null;
});

call("api/account")
  .then((account) => {
    show(account);
    if (account.google_client_id) loadGoogle(account.google_client_id);
  })
  .catch(() => show({ signed_in: false, google_client_id: null }));
