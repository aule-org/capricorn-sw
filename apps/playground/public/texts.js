// The words of the sign-in and of the one-field student sign-up, in the two
// languages Aule speaks with a family. The page's own Spanish stays in the HTML,
// so it reads the same before this script runs; the same words are here for
// `applyTexts` and for what the script says.

export const LANGUAGES = ["es", "en"];

export const TEXTS = {
  es: {
    "signin.title": "Entra a tu sesión",
    "mail.by": "Continuar con tu correo",
    "mail.email": "Tu correo",
    "mail.send": "Enviarme un código",
    "mail.code": "Código de 6 dígitos",
    // Said after asking, whatever happened: it neither confirms a mail was sent
    // nor shows whether the address has an account.
    "mail.sent":
      "Si el correo es correcto, te llegará un código en un minuto. Si no llega, revisa la carpeta de spam o vuelve a intentarlo en unos minutos.",
    "mail.name": "¿Cómo te llamamos?",
    "mail.enter": "Entrar",
    "mail.again": "Usar otro correo",
    "mail.first": "La primera vez se crea tu cuenta de padre de familia.",
    "mail.unavailable": "Ahora no podemos enviar códigos por correo. Prueba con Google.",
    "mail.unavailableOnly": "Ahora no podemos enviar códigos por correo.",
    "mail.bad": "Revisa tu correo e inténtalo otra vez.",
    "mail.failed": "No se pudo enviar el código. Inténtalo otra vez.",
    "mail.nameNeeded": "Es tu primera vez. Dinos cómo te llamamos y vuelve a pulsar Entrar.",
    "mail.refused": "Ese código no es válido o ya venció. Pide uno nuevo.",
    "mail.enterFailed": "No se pudo entrar con ese código. Inténtalo otra vez.",
    "student.userName": "Nombre de usuario del alumno",
    "student.userNameHint": "Un apodo inventado. No uses su nombre real.",
    "student.userNameMissing": "Escribe el nombre de usuario del alumno.",
  },
  en: {
    "signin.title": "Sign in to your session",
    "mail.by": "Continue with your email",
    "mail.email": "Your email",
    "mail.send": "Email me a code",
    "mail.code": "6-digit code",
    "mail.sent":
      "If the address is right, a code will reach you in a minute. If it doesn't arrive, check your spam folder or try again in a few minutes.",
    "mail.name": "What should we call you?",
    "mail.enter": "Sign in",
    "mail.again": "Use another email",
    "mail.first": "Your parent account is created the first time you sign in.",
    "mail.unavailable": "We can't send codes by email right now. Try Google.",
    "mail.unavailableOnly": "We can't send codes by email right now.",
    "mail.bad": "Check your email address and try again.",
    "mail.failed": "The code could not be requested. Try again.",
    "mail.nameNeeded": "This is your first time. Tell us what to call you and press Sign in again.",
    "mail.refused": "That code is not valid or has expired. Ask for a new one.",
    "mail.enterFailed": "Could not sign in with that code. Try again.",
    "student.userName": "Student's username",
    "student.userNameHint": "A made-up nickname. Don't use their real name.",
    "student.userNameMissing": "Write the student's username.",
  },
};

/** The first language the browser asks for that Aule speaks; Spanish when none. */
export function pickLanguage(preferred) {
  for (const tag of preferred ?? []) {
    const language = String(tag).toLowerCase().split("-")[0];
    if (LANGUAGES.includes(language)) return language;
  }
  return "es";
}

/** The page's language, from the browser. */
export const pageLanguage = () => pickLanguage(globalThis.navigator?.languages);

export const text = (language, key) => TEXTS[language]?.[key] ?? TEXTS.es[key];

/** Writes the marked texts of the page in a language. */
export function applyTexts(root, language) {
  for (const element of root.querySelectorAll("[data-t]")) {
    element.textContent = text(language, element.dataset.t);
  }
}
