"""How a session opens: a greeting and one question to break the ice.

Written and read by people, like the fixed replies: the first words a student
hears do not need a model, and saying them at once is two to three seconds
sooner than writing them. There are enough of them, taken in turn, that a
student who comes three times a week does not hear the same one for a month.
"""

import random

# "{hello}" becomes the greeting with the student's name when it is known, and
# "{tutor}" the name the family chose for the tutor.
GREETINGS: dict[str, tuple[str, ...]] = {
    "es": (
        "{hello} Soy {tutor}, qué gusto verte. Cuéntame, ¿cómo te fue hoy?",
        "{hello} Aquí {tutor}. ¿Qué fue lo mejor de tu día?",
        "{hello} Soy {tutor}. Antes de empezar, cuéntame: ¿cómo va tu semana?",
        "{hello} Qué bueno que llegaste. Soy {tutor}. ¿Qué hiciste hoy en el cole?",
        "{hello} Soy {tutor} y me alegra verte. ¿Cómo te sientes hoy?",
        "{hello} Aquí estoy, soy {tutor}. Cuéntame algo bueno que te haya pasado hoy.",
        "{hello} Soy {tutor}. ¿Qué tal estuvo tu día, tranquilo o movido?",
        "{hello} Qué gusto, soy {tutor}. ¿Jugaste a algo hoy?",
        "{hello} Soy {tutor}. Antes de trabajar, dime: ¿qué fue lo más divertido de hoy?",
        "{hello} Me alegra que estés aquí, soy {tutor}. ¿Cómo te fue en el cole?",
    ),
    "en": (
        "{hello} I'm {tutor}, good to see you. Tell me, how did today go?",
        "{hello} {tutor} here. What was the best part of your day?",
        "{hello} I'm {tutor}. Before we start, tell me: how is your week going?",
        "{hello} Glad you made it. I'm {tutor}. What did you do at school today?",
        "{hello} I'm {tutor} and I'm happy to see you. How are you feeling today?",
        "{hello} Here I am, I'm {tutor}. Tell me one good thing that happened today.",
        "{hello} I'm {tutor}. How was your day, quiet or busy?",
        "{hello} Good to see you, I'm {tutor}. Did you play anything today?",
        "{hello} I'm {tutor}. Before we work, tell me: what was the most fun today?",
        "{hello} I'm glad you're here, I'm {tutor}. How did school go?",
    ),
}

_HELLO = {"es": ("¡Hola!", "¡Hola, {name}!"), "en": ("Hi!", "Hi, {name}!")}


def greeting(
    language: str, tutor: str, name: str | None = None, session_number: int | None = None
) -> str:
    """The greeting for this session.

    With the number of the student's session they are taken in turn, so two
    sessions in a row never open the same way; without it, one at random.
    """
    choices = GREETINGS[language]
    index = session_number % len(choices) if session_number is not None else None
    chosen = choices[index] if index is not None else random.choice(choices)  # noqa: S311
    plain, named = _HELLO[language]
    hello = named.format(name=name) if name else plain
    return chosen.format(hello=hello, tutor=tutor)
