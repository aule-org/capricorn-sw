// The playground page: asks the server for a room token, joins the room with
// the microphone and the camera, and shows what the tutor is doing.

const { Room, RoomEvent, Track, ParticipantKind } = LivekitClient;

const TRANSCRIPTION_TOPIC = "lk.transcription";
const AGENT_STATE = "lk.agent.state";
const TUTOR_WAIT_MS = 15_000;

const STATE_TEXT = {
  waiting: "Waiting for the tutor…",
  initializing: "The tutor is getting ready…",
  listening: "Listening",
  thinking: "Thinking…",
  speaking: "Speaking",
  missing:
    "The tutor did not join. The agent worker is probably not running or has no provider keys.",
  left: "The tutor left the session.",
};

const ERRORS = {
  wrong_code: "That access code is not right.",
  too_many_attempts: "Too many wrong codes. Wait a minute and try again.",
};

const $ = (id) => document.getElementById(id);
const entry = $("entry");
const stage = $("stage");
const message = $("message");

let room = null;
let clock = null;
let tutorTimer = null;

function showMessage(text) {
  message.textContent = text;
  message.hidden = !text;
}

function setTutorState(state) {
  $("tutor").dataset.state = state;
  $("tutor-state").textContent = STATE_TEXT[state] ?? state;
}

function setToggle(button, label, on) {
  button.setAttribute("aria-pressed", String(on));
  button.textContent = `${label} ${on ? "on" : "off"}`;
}

function startClock(seconds) {
  const endsAt = Date.now() + seconds * 1000;
  const tick = () => {
    const left = Math.max(0, Math.round((endsAt - Date.now()) / 1000));
    $("clock").textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
    if (left === 0) leave("The session reached its time limit.");
  };
  tick();
  clock = setInterval(tick, 1000);
}

// One line per transcription segment; a segment is streamed, so its line grows.
const lines = new Map();

async function onTranscription(reader, participant) {
  const attributes = reader.info.attributes ?? {};
  const id = attributes["lk.segment_id"] ?? reader.info.id;
  const local = room?.localParticipant;
  // The agent may publish the person's words itself, naming the track it heard.
  const mine =
    participant.identity === local?.identity ||
    [...(local?.trackPublications.values() ?? [])].some(
      (publication) => publication.trackSid === attributes["lk.transcribed_track_id"],
    );
  let line = lines.get(id);
  if (!line) {
    line = document.createElement("li");
    line.dataset.who = mine ? "you" : "tutor";
    line.dataset.name = mine ? "You" : "Capri";
    lines.set(id, line);
    $("transcript").append(line);
  }
  // The person's speech arrives as whole revisions; the tutor's as increments.
  let text = "";
  for await (const chunk of reader) {
    text += chunk;
    line.textContent = text;
    line.scrollIntoView({ block: "nearest" });
  }
}

function watchTutor(participant) {
  if (participant.kind !== ParticipantKind.AGENT) return;
  clearTimeout(tutorTimer);
  setTutorState(participant.attributes[AGENT_STATE] ?? "initializing");
}

function showCamera() {
  const publication = room.localParticipant.getTrackPublication(Track.Source.Camera);
  const on = Boolean(publication?.track) && !publication.isMuted;
  if (on) publication.track.attach($("camera"));
  $("camera").hidden = !on;
  $("camera-off").hidden = on;
  setToggle($("cam"), "Camera", on);
}

async function join(session) {
  room = new Room({ adaptiveStream: true, dynacast: true });

  room
    .on(RoomEvent.ParticipantConnected, watchTutor)
    .on(RoomEvent.ParticipantAttributesChanged, (_changed, participant) => watchTutor(participant))
    .on(RoomEvent.ParticipantDisconnected, (participant) => {
      if (participant.kind === ParticipantKind.AGENT) setTutorState("left");
    })
    .on(RoomEvent.TrackSubscribed, (track) => {
      if (track.kind === Track.Kind.Audio) $("audio").append(track.attach());
    })
    .on(RoomEvent.TrackUnsubscribed, (track) => track.detach().forEach((el) => el.remove()))
    .on(RoomEvent.LocalTrackPublished, showCamera)
    .on(RoomEvent.TrackMuted, showCamera)
    .on(RoomEvent.TrackUnmuted, showCamera)
    .on(RoomEvent.Disconnected, () => leave("The session ended."));
  room.registerTextStreamHandler(TRANSCRIPTION_TOPIC, (reader, participant) => {
    onTranscription(reader, participant).catch(() => undefined);
  });

  await room.connect(session.serverUrl, session.token);
  await room.startAudio();
  room.remoteParticipants.forEach(watchTutor);

  setTutorState("waiting");
  tutorTimer = setTimeout(() => setTutorState("missing"), TUTOR_WAIT_MS);
  startClock(session.expiresInSeconds);
  entry.hidden = true;
  stage.hidden = false;

  await room.localParticipant.setMicrophoneEnabled(true);
  setToggle($("mic"), "Microphone", true);
  try {
    await room.localParticipant.setCameraEnabled(true);
  } catch {
    // No camera, or the person said no: the tutor still works by voice.
  }
  showCamera();
}

function leave(reason) {
  if (!room) return;
  const leaving = room;
  room = null;
  clearInterval(clock);
  clearTimeout(tutorTimer);
  leaving.disconnect();
  $("audio").replaceChildren();
  stage.hidden = true;
  entry.hidden = false;
  $("start").disabled = false;
  showMessage(reason ?? "");
}

entry.addEventListener("submit", async (event) => {
  event.preventDefault();
  showMessage("");
  $("start").disabled = true;
  $("transcript").replaceChildren();
  lines.clear();
  try {
    const response = await fetch("api/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: $("code").value, language: $("language").value }),
    });
    const body = await response.json();
    if (!response.ok) throw new Error(ERRORS[body.error] ?? "The session could not be started.");
    await join(body);
  } catch (error) {
    const text =
      error?.name === "NotAllowedError"
        ? "The browser blocked the microphone. Allow it and start again."
        : (error?.message ?? "The session could not be started.");
    if (room) leave(text);
    else showMessage(text);
    $("start").disabled = false;
  }
});

$("mic").addEventListener("click", async () => {
  const on = !room.localParticipant.isMicrophoneEnabled;
  await room.localParticipant.setMicrophoneEnabled(on);
  setToggle($("mic"), "Microphone", on);
});

$("cam").addEventListener("click", async () => {
  await room.localParticipant.setCameraEnabled(!room.localParticipant.isCameraEnabled);
  showCamera();
});

$("end").addEventListener("click", () => leave());
