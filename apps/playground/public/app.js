// The playground page: lets the person check their camera and microphone,
// asks the server for a room token, joins the room, and shows what the tutor
// is doing.

import { TOPIC as VISUAL_TOPIC, renderVisual } from "./visuals.js";

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

const SUPERVISOR_POLL_MS = 3000;

const ERRORS = {
  wrong_code: "That access code is not right.",
  too_many_attempts: "Too many wrong codes. Wait a minute and try again.",
};

const $ = (id) => document.getElementById(id);
const entry = $("entry");
const lobby = $("lobby");
const stage = $("stage");
const message = $("message");

// What the person chose before joining. Both start on.
const wanted = { mic: true, cam: true };
let preview = null;
let meter = null;

let room = null;
let clock = null;
let tutorTimer = null;
let supervisorTimer = null;

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

function item(text) {
  const element = document.createElement("li");
  element.textContent = text;
  return element;
}

// Shows the supervisor's plan and the guidance it gives the tutor, when the
// session was created by a platform.
async function showSupervisor(sessionId) {
  try {
    const response = await fetch("api/supervisor", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: $("code").value, session_id: sessionId }),
    });
    if (!response.ok) return;
    const view = await response.json();
    $("supervisor").hidden = false;
    $("plan").textContent = view.brief?.goal ?? "No plan for this session.";
    $("plan-skills").replaceChildren(
      ...(view.brief?.skills ?? []).map((skill) => item(`${skill.skill_id}: ${skill.start_with}`)),
    );
    $("notes").replaceChildren(...(view.guidance ?? []).map((note) => item(note.text)));
  } catch {
    // The panel is a view of the session, not part of it.
  }
}

// The tutor describes what to show; the page draws it.
async function onVisual(reader) {
  try {
    $("board").innerHTML = renderVisual(JSON.parse(await reader.readAll()));
    $("board").hidden = false;
  } catch {
    // A description this page cannot draw is skipped; the voice carries on.
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

  room.registerTextStreamHandler(VISUAL_TOPIC, (reader) => {
    onVisual(reader);
  });

  await room.connect(session.serverUrl, session.token);
  await room.startAudio();
  room.remoteParticipants.forEach(watchTutor);

  setTutorState("waiting");
  tutorTimer = setTimeout(() => setTutorState("missing"), TUTOR_WAIT_MS);
  startClock(session.expiresInSeconds);
  // The supervisor's plan is for the team. It stays off the screen unless the
  // person asked for it, so a child never reads what the tutor is aiming at.
  if (session.session_id && $("operator").checked) {
    showSupervisor(session.session_id);
    supervisorTimer = setInterval(() => showSupervisor(session.session_id), SUPERVISOR_POLL_MS);
  }
  lobby.hidden = true;
  stage.hidden = false;

  await room.localParticipant.setMicrophoneEnabled(wanted.mic);
  setToggle($("mic"), "Microphone", wanted.mic);
  try {
    await room.localParticipant.setCameraEnabled(wanted.cam);
  } catch {
    // No camera, or the person said no: the tutor still works by voice.
  }
  showCamera();
}

function stopPreview() {
  cancelAnimationFrame(meter?.frame);
  meter?.context.close().catch(() => undefined);
  meter = null;
  preview?.getTracks().forEach((track) => track.stop());
  preview = null;
  $("preview").srcObject = null;
}

function showPreview() {
  const video = preview?.getVideoTracks()[0];
  const audio = preview?.getAudioTracks()[0];
  if (video) video.enabled = wanted.cam;
  if (audio) audio.enabled = wanted.mic;
  const seen = Boolean(video) && wanted.cam;
  $("preview").hidden = !seen;
  $("preview-off").hidden = seen;
  $("level").hidden = !(audio && wanted.mic);
  setToggle($("lobby-mic"), "Microphone", wanted.mic);
  setToggle($("lobby-cam"), "Camera", wanted.cam);
}

// Moves a bar with the loudness of the microphone, so the person can tell it
// hears them before they join.
function startMeter(stream) {
  const context = new AudioContext();
  const analyser = context.createAnalyser();
  analyser.fftSize = 512;
  context.createMediaStreamSource(stream).connect(analyser);
  const samples = new Uint8Array(analyser.fftSize);
  const bar = $("level").firstElementChild;
  meter = { context, frame: 0 };
  const draw = () => {
    analyser.getByteTimeDomainData(samples);
    let peak = 0;
    for (const sample of samples) peak = Math.max(peak, Math.abs(sample - 128));
    bar.style.width = `${Math.min(100, Math.round((peak / 128) * 250))}%`;
    if (meter) meter.frame = requestAnimationFrame(draw);
  };
  draw();
}

async function openLobby() {
  wanted.mic = true;
  wanted.cam = true;
  $("lobby-note").textContent = "";
  entry.hidden = true;
  lobby.hidden = false;
  // Joining waits until the devices have answered, so only one thing holds them.
  $("join").disabled = true;
  let usable = true;
  try {
    preview = await navigator.mediaDevices.getUserMedia({ audio: true, video: true });
  } catch {
    // No camera, or only the camera was refused: try the microphone alone.
    wanted.cam = false;
    try {
      preview = await navigator.mediaDevices.getUserMedia({ audio: true });
      $("lobby-note").textContent = "No camera was found or allowed. You can still join by voice.";
    } catch {
      wanted.mic = false;
      $("lobby-note").textContent =
        "The browser blocked the microphone. Allow it in the address bar and go back to try again.";
      usable = false;
    }
  }
  if (lobby.hidden) return stopPreview();
  $("join").disabled = !usable;
  if (preview) {
    $("preview").srcObject = preview;
    if (preview.getAudioTracks().length) startMeter(preview);
  }
  showPreview();
}

function closeLobby() {
  stopPreview();
  lobby.hidden = true;
  entry.hidden = false;
}

function leave(reason) {
  if (!room) return;
  const leaving = room;
  room = null;
  clearInterval(clock);
  clearTimeout(tutorTimer);
  clearInterval(supervisorTimer);
  $("supervisor").hidden = true;
  leaving.disconnect();
  $("audio").replaceChildren();
  $("board").replaceChildren();
  $("board").hidden = true;
  stage.hidden = true;
  lobby.hidden = true;
  entry.hidden = false;
  showMessage(reason ?? "");
}

entry.addEventListener("submit", (event) => {
  event.preventDefault();
  showMessage("");
  openLobby();
});

$("lobby-mic").addEventListener("click", () => {
  wanted.mic = !wanted.mic;
  showPreview();
});

$("lobby-cam").addEventListener("click", () => {
  if (!preview?.getVideoTracks().length) return;
  wanted.cam = !wanted.cam;
  showPreview();
});

$("back").addEventListener("click", closeLobby);

$("join").addEventListener("click", async () => {
  $("join").disabled = true;
  $("transcript").replaceChildren();
  lines.clear();
  try {
    const response = await fetch("api/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: $("code").value, language: $("language").value }),
    });
    const body = await response.json();
    if (!response.ok) {
      throw new Error(ERRORS[body.error] ?? body.message ?? "The session could not be started.");
    }
    // The room opens the devices itself, with what was chosen here.
    stopPreview();
    await join(body);
  } catch (error) {
    const text =
      error?.name === "NotAllowedError"
        ? "The browser blocked the microphone. Allow it and start again."
        : (error?.message ?? "The session could not be started.");
    if (room) leave(text);
    else {
      closeLobby();
      showMessage(text);
    }
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
