from livekit import rtc
from livekit.agents import llm

from capricorn_agent.tutor import INSTRUCTIONS, with_frame
from capricorn_agent.worker import GOODBYE, language_of


def frame():
    return rtc.VideoFrame(2, 2, rtc.VideoBufferType.RGBA, bytes(16))


def context():
    chat_ctx = llm.ChatContext()
    chat_ctx.add_message(role="user", content="mira mi cuaderno")
    chat_ctx.add_message(role="assistant", content="¿qué intentaste?")
    chat_ctx.add_message(role="user", content="esto")
    return chat_ctx


def images(message):
    return [part for part in message.content if isinstance(part, llm.ImageContent)]


def test_the_frame_goes_on_the_last_user_message_only():
    seen = with_frame(context(), frame())

    first, _, last = seen.items
    assert images(first) == []
    assert len(images(last)) == 1
    assert last.text_content == "esto"


def test_the_session_history_is_left_without_images():
    chat_ctx = context()

    with_frame(chat_ctx, frame())

    assert all(images(item) == [] for item in chat_ctx.items)


def test_without_a_frame_or_a_user_message_the_context_is_unchanged():
    chat_ctx = context()
    assert with_frame(chat_ctx, None) is chat_ctx

    empty = llm.ChatContext()
    assert with_frame(empty, frame()) is empty


def test_every_language_has_its_texts():
    assert set(INSTRUCTIONS) == set(GOODBYE) == {"es", "en"}


def test_language_comes_from_the_dispatch_when_supported():
    assert language_of('{"language": "en"}', "es") == "en"
    assert language_of('{"language": "fr"}', "es") == "es"
    assert language_of("not json", "es") == "es"
    assert language_of("", "en") == "en"
