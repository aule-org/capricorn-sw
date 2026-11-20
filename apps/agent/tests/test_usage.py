from types import SimpleNamespace

from aule_agent.usage import Usage


def test_a_session_adds_up_its_model_calls():
    usage = Usage()
    usage.add(SimpleNamespace(prompt_tokens=3466, prompt_cached_tokens=3310, completion_tokens=66))
    usage.add(SimpleNamespace(prompt_tokens=3500, prompt_cached_tokens=0, completion_tokens=40))

    assert usage.summary() == {
        "llm_calls": 2,
        "input_tokens": 6966,
        "cached_input_tokens": 3310,
        "output_tokens": 106,
    }


def test_metrics_without_the_expected_counts_add_nothing_but_the_call():
    usage = Usage()
    usage.add(SimpleNamespace(prompt_tokens=None, completion_tokens=-1))

    assert usage.summary() == {
        "llm_calls": 1,
        "input_tokens": 0,
        "cached_input_tokens": 0,
        "output_tokens": 0,
    }
