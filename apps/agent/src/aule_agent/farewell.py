"""Whether the student just said goodbye.

A child who says "chau" expects the tutor to say it back and leave, not to
carry on with the lesson. Only a clear goodbye counts, in a short turn: "me voy
a equivocar" is not one, and neither is a story that happens to have "adiós" in
it. Leaving counts only as the last thing said.
"""

import unicodedata

# A turn longer than this is about something else.
MAX_WORDS = 12

WORDS = {"chau", "chao", "chaito", "chaucito", "adios", "bye", "bai", "goodbye", "byebye"}
PHRASES = (
    "hasta luego",
    "hasta manana",
    "hasta la proxima",
    "hasta pronto",
    "nos vemos",
    "see you",
    "see ya",
)
LEAVING = (
    "me voy",
    "ya me voy",
    "me tengo que ir",
    "tengo que irme",
    "me voy a dormir",
    "me voy a comer",
    "me voy a cenar",
    "me voy a almorzar",
    "i have to go",
    "i gotta go",
    "i need to go",
    "im leaving",
    "i am leaving",
    "im going now",
)


def _plain(text: str) -> list[str]:
    """Lower case, no accents, letters and digits only, as words."""
    unaccented = unicodedata.normalize("NFKD", text.lower())
    letters = "".join(c if c.isalnum() else " " for c in unaccented if not unicodedata.combining(c))
    return letters.split()


def said_goodbye(text: str) -> bool:
    words = _plain(text)
    if not words or len(words) > MAX_WORDS:
        return False
    if any(word in WORDS for word in words):
        return True
    said = f" {' '.join(words)} "
    if any(f" {phrase} " in said for phrase in PHRASES):
        return True
    return any(said.endswith(f" {leaving} ") for leaving in LEAVING)
