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
    # A student who says something out of place is brought back to the work,
    # with no other plan offered: a game here would reward it.
    "redirect": {
        "es": "De eso no puedo hablar. Sigamos con lo que estábamos haciendo.",
        "en": "I can't talk about that. Let's go on with what we were doing.",
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
    # The three below end on going back to the work. Worded as "afterwards",
    # not "once you are safe": the gate reads the second as a danger still
    # open and then stops a plain "let's carry on" (measured with the cases
    # in evals/physical-safety).
    "adult_help": {
        "es": "Primero, pide ayuda a un adulto de confianza que esté cerca. No lo intentes tú. "
        "Después seguimos con lo que estábamos haciendo.",
        "en": "First, ask a trusted adult nearby for help. Don't try it yourself. Afterwards we "
        "go on with what we were doing.",
    },
    "adult_help_again": {
        "es": "No lo intentes tú; pide ayuda a un adulto de confianza cercano. Después seguimos.",
        "en": "Don't try it yourself; ask a trusted adult nearby for help. Afterwards we go on.",
    },
    "adult_help_wait": {
        "es": "Busca a un adulto de confianza que esté cerca; deja que se encargue. Después "
        "seguimos con lo nuestro.",
        "en": "Find a trusted adult nearby and let them handle it. Afterwards we go on with our "
        "work.",
    },
    "clarify": {
        "es": "No estoy seguro de qué quieres hacer. ¿Me lo cuentas de otra manera?",
        "en": "I'm not sure what you want to do. Can you tell me another way?",
    },
    "context_limit": {
        "es": "Necesito ayuda de un adulto para seguir con seguridad. Pídele que revise lo que "
        "pasa y vuelva a iniciar la sesión.",
        "en": "I need an adult's help to continue safely. Ask them to check what's happening and "
        "start the session again.",
    },
}

# Consecutive physical hazards get the full reply first and then alternate two
# shorter reminders. Each one stands alone: none assumes the hazard is the same
# one or that it is over.
ADULT_HELP_SEQUENCE = ("adult_help", "adult_help_again", "adult_help_wait")

# Replies the following turn may answer ("ya vino mi mamá", the same thing said
# another way). They stay in the conversation so that it has something to
# refer to.
ANSWERABLE = frozenset({*ADULT_HELP_SEQUENCE, "clarify"})


def reply(key: str, language: str) -> str:
    return REPLIES[key][language]
