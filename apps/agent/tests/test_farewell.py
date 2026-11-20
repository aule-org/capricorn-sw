import pytest

from aule_agent.farewell import said_goodbye


@pytest.mark.parametrize(
    "said",
    [
        "Chau",
        "¡Adiós, Aule!",
        "ya, chaito",
        "bueno, hasta mañana",
        "nos vemos",
        "ya me voy",
        "Me tengo que ir.",
        "me voy a dormir",
        "bye bye",
        "OK, see you tomorrow",
        "I have to go",
    ],
)
def test_a_clear_goodbye_is_one(said):
    assert said_goodbye(said)


@pytest.mark.parametrize(
    "said",
    [
        "",
        "me voy a equivocar",
        "¿cuánto es tres cuartos más un cuarto?",
        "no me quiero ir",
        "ya terminé el ejercicio",
        "mi abuela dice adiós a todos los que pasan por la calle cuando sale a comprar pan",
        "chaucha",
    ],
)
def test_anything_else_is_not(said):
    assert not said_goodbye(said)
