from aule_agent.script import MAX_LINE_CHARS, MAX_LINES, Line, Reply, lines_from, reply_from

DRAWING = {"kind": "dot_array", "rows": 2, "columns": 3}


def test_lines_are_read_in_order_and_a_broken_one_is_left_out():
    lines = lines_from(
        [
            {"say": " Hola. ", "show": None},
            {"say": ""},
            "not a line",
            {"say": 7},
            {"say": "x" * (MAX_LINE_CHARS + 1)},
            {"say": "Mira.", "show": DRAWING},
        ]
    )

    assert lines == (Line("Hola."), Line("Mira.", show=DRAWING))
    assert lines_from("nope") == () and lines_from(None) == ()


def test_a_drawing_the_screen_cannot_draw_is_dropped_and_its_line_kept():
    assert lines_from([{"say": "Mira.", "show": {"kind": "hologram"}}]) == (Line("Mira."),)


def test_no_more_lines_than_a_turn_can_hold():
    assert len(lines_from([{"say": "a"}] * (MAX_LINES + 5))) == MAX_LINES


def test_a_reply_is_lines_rules_for_talking_or_both_and_nothing_is_none():
    assert reply_from({"lines": [{"say": "Bien."}]}) == Reply(lines=(Line("Bien."),))
    assert reply_from({"lines": [], "converse": " No hints. "}) == Reply(converse="No hints.")
    assert reply_from({"lines": [], "converse": None}) is None
    assert reply_from({}) is None and reply_from(None) is None and reply_from([]) is None
