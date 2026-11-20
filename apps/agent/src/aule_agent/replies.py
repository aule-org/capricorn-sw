"""The fixed replies a person hears when a safety gate stops a turn, and when
a session is stopped because the student is not working.

They are written and reviewed by people, in every supported language, and are
never generated or translated by a model: what is said after a gate stops a
turn must not depend on the thing that was just stopped.
"""

REPLIES: dict[str, dict[str, str]] = {
    # Said once, when the student keeps fooling around: a session never ends
    # without a warning.
    "not_working_warn": {
        "es": "Me parece que hoy no tenemos muchas ganas de trabajar, y no pasa nada. Pero si "
        "seguimos así, lo dejamos por hoy y le aviso a tu familia. ¿Lo intentamos una vez más?",
        "en": "It seems we don't feel much like working today, and that's all right. But if we "
        "carry on like this, we stop for today and I let your family know. Shall we try once more?",
    },
    "not_working_end": {
        "es": "Lo dejamos aquí por hoy. Le aviso a tu familia para buscar otro momento. "
        "¡Nos vemos pronto!",
        "en": "We stop here for today. I'll let your family know so we can find another time. "
        "See you soon!",
    },
    "redirect": {
        "es": "Con eso no puedo ayudarte. Podemos pensar en cómo cuidarnos o inventar una "
        "aventura juntos.",
        "en": "I can't help with that. We can talk about staying safe or make up an adventure "
        "together.",
    },
    "child_support": {
        "es": "Gracias por contármelo. Si algo te hizo sentir incómodo o en peligro, busca a un "
        "adulto de confianza que pueda ayudarte. No tienes que resolverlo a solas.",
        "en": "Thank you for telling me. If something made you feel uncomfortable or unsafe, "
        "find a trusted adult who can help. You don't have to handle it alone.",
    },
    "self_harm_support": {
        "es": "Siento que estés pasando por algo tan difícil. Cuéntaselo ahora mismo a un adulto "
        "de confianza que esté cerca; si hay peligro inmediato, pídele que contacte a "
        "emergencias.",
        "en": "I'm sorry things feel so difficult. Tell a trusted adult nearby right now; if "
        "there is immediate danger, ask them to contact emergency services.",
    },
    "unavailable": {
        "es": "Ahora mismo no puedo responder con seguridad. Inténtalo de nuevo en un momento o "
        "pide ayuda a un adulto de confianza.",
        "en": "I can't respond safely right now. Try again in a moment or ask a trusted adult "
        "for help.",
    },
    "adult_help": {
        "es": "Primero, pide ayuda a un adulto de confianza que esté cerca. No lo intentes tú. "
        "Después podemos jugar una adivinanza o inventar una aventura. ¿Qué prefieres?",
        "en": "First, ask a trusted adult nearby for help. Don't try it yourself. Afterwards we "
        "can play a guessing game or make up an adventure. Which would you like?",
    },
    "adult_help_riddle": {
        "es": "No lo intentes tú; pide ayuda a un adulto de confianza cercano. Cuando estés a "
        "salvo, podemos jugar una adivinanza.",
        "en": "Don't try it yourself; ask a trusted adult nearby for help. Once you're safe, we "
        "can play a guessing game.",
    },
    "adult_help_story": {
        "es": "Busca a un adulto de confianza que esté cerca; deja que se encargue. Cuando estés "
        "a salvo, podemos inventar un cuento.",
        "en": "Find a trusted adult nearby and let them handle it. Once you're safe, we can make "
        "up a story.",
    },
    "clarify": {
        "es": "No estoy seguro de qué quieres hacer. ¿Me lo cuentas de otra manera? También "
        "podemos jugar una adivinanza o inventar un cuento.",
        "en": "I'm not sure what you want to do. Can you tell me another way? We can also play a "
        "guessing game or make up a story.",
    },
    "context_limit": {
        "es": "Necesito ayuda de un adulto para seguir con seguridad. Pídele que revise lo que "
        "pasa y vuelva a iniciar la sesión.",
        "en": "I need an adult's help to continue safely. Ask them to check what's happening and "
        "start the session again.",
    },
}

# Consecutive physical hazards get the full offer first and then alternate two
# shorter reminders. Each one stands alone: none assumes the hazard is the same
# one or that it is over.
ADULT_HELP_SEQUENCE = ("adult_help", "adult_help_riddle", "adult_help_story")

# Replies that offer something to do next. They stay in the conversation so
# that "the riddle" has something to refer to on the following turn.
OFFERS = frozenset({*ADULT_HELP_SEQUENCE, "clarify"})


def reply(key: str, language: str) -> str:
    return REPLIES[key][language]
