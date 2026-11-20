// The playground page: lets the person check their camera and microphone,
// asks the server for a room token, joins the room, and shows what the tutor
// is doing.

import { onEnter, sessionRequest } from "./signin.js";
import { TOPIC as VISUAL_TOPIC, renderVisual } from "./visuals.js";

const { Room, RoomEvent, Track, ParticipantKind, DisconnectReason } = LivekitClient;

const TRANSCRIPTION_TOPIC = "lk.transcription";
const AGENT_STATE = "lk.agent.state";
// Set by the tutor when a turn lasts while a button is held, like on the
// device, instead of ending when the person goes quiet.
const TURNS = "capricorn.turns";
// How long the microphone stays open after the button is let go, so the last
// syllable is not cut.
const TALK_TAIL_MS = 200;
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
  reconnecting: "The connection was lost. Reconnecting…",
};

// Ways a session ends that nobody asked for.
const LOST = new Set([
  DisconnectReason.JOIN_FAILURE,
  DisconnectReason.SIGNAL_CLOSE,
  DisconnectReason.STATE_MISMATCH,
  DisconnectReason.CONNECTION_TIMEOUT,
  DisconnectReason.MEDIA_FAILURE,
]);

const SUPERVISOR_POLL_MS = 3000;
// The review is written after the session ends; it usually takes under a
// minute. After this long the page stops asking.
const REPORT_WAIT_MS = 120_000;
// Typed messages go to the tutor on the topic LiveKit agents read.
const CHAT_TOPIC = "lk.chat";

const ERRORS = {
  wrong_code: "That access code is not right.",
  too_many_attempts: "Too many wrong codes. Wait a minute and try again.",
  daily_limit: "Ya usaste las sesiones de hoy. Vuelve mañana.",
  not_signed_in: "Tu sesión se cerró. Vuelve a entrar con Google.",
};

const $ = (id) => document.getElementById(id);
// Everything shown before a session: signing in, and the team's access code.
const entry = $("welcome");
const lobby = $("lobby");
const stage = $("stage");
const message = $("message");

// What the person chose before joining. Both start on.
const wanted = { mic: true, cam: true };
let preview = null;
let meter = null;

// Push-to-talk, when the tutor works that way.
let tutorIdentity = null;
// True once the tutor is in the session and has said how turns work. Until
// then the microphone stays closed: nobody is listening yet, and the page does
// not know whether it should be open at all.
let tutorReady = false;
let manualTurns = false;
let talking = false;

let room = null;
let clock = null;
let tutorTimer = null;
let supervisorTimer = null;
let reportTimer = null;
// The platform's id for the session being shown, when there is one.
let sessionId = null;

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
    line.dataset.name = mine ? "You" : "Aule";
    lines.set(id, line);
    $("transcript").append(line);
  }
  // The person's speech arrives as whole revisions; the tutor's as increments.
  let text = "";
  for await (const chunk of reader) {
    text += chunk;
    // The page stays where the person left it; only the conversation's own
    // column follows, and only while it is already at its end.
    const list = $("transcript");
    const atEnd = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
    line.textContent = text;
    if (atEnd) list.scrollTop = list.scrollHeight;
  }
}

function item(text) {
  const element = document.createElement("li");
  element.textContent = text;
  return element;
}

// Shows the supervisor's plan and the guidance it gives the tutor, when the
// session was created by a platform.
async function supervisorView(id) {
  const response = await fetch("api/supervisor", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code: $("code").value, session_id: id }),
  });
  return response.ok ? response.json() : null;
}

async function showSupervisor(id) {
  try {
    const view = await supervisorView(id);
    if (!view) return;
    $("supervisor").hidden = false;
    $("plan").textContent = view.brief?.goal ?? "No plan for this session.";
    $("plan-skills").replaceChildren(
      ...(view.brief?.skills ?? []).map((skill) => item(`${skill.skill_id}: ${skill.start_with}`)),
    );
    $("notes").replaceChildren(...(view.guidance ?? []).map((note) => item(note.text)));
    // What the platform worked out and checked. The answers are for the team.
    const checked = view.exercises ?? [];
    $("checked-title").hidden = checked.length === 0;
    $("checked").replaceChildren(
      ...checked.map((exercise) =>
        item(
          [
            `${exercise.probe ? "Finding out" : "Practice"}: ${exercise.expression} = ${exercise.answer}`,
            exercise.status,
            `${exercise.attempts} ${exercise.attempts === 1 ? "try" : "tries"}, ${exercise.hints} ${exercise.hints === 1 ? "hint" : "hints"}`,
            exercise.misconception ? `mistake: ${exercise.misconception.replaceAll("_", " ")}` : "",
          ]
            .filter(Boolean)
            .join(" · "),
        ),
      ),
    );
  } catch {
    // The panel is a view of the session, not part of it.
  }
}

// After a session, shows what the review wrote for the family. The review
// runs once the session has ended, so the page asks until it is there.
function awaitReport(id) {
  const started = Date.now();
  $("report").hidden = false;
  $("report-summary").textContent = "";
  $("report-next").hidden = true;
  $("report-note").textContent = "The session is being reviewed…";
  const ask = async () => {
    let view = null;
    try {
      view = await supervisorView(id);
    } catch {
      // Asked again on the next round.
    }
    if (view?.review) {
      $("report-summary").textContent = view.review.guardian_summary;
      // The next step is a note for whoever plans the next session, not for the family.
      $("report-next").textContent = view.review.next_step
        ? `Note for the next session's plan (not sent to the family): ${view.review.next_step}`
        : "";
      $("report-next").hidden = !view.review.next_step;
      $("report-note").textContent = "The summary above is what the family reads. The plan and the conversation are not sent.";
    } else if (Date.now() - started < REPORT_WAIT_MS) {
      reportTimer = setTimeout(ask, SUPERVISOR_POLL_MS);
    } else {
      $("report-note").textContent = "No report was written for this session.";
    }
  };
  ask();
}

// The tutor describes what to show; the page draws it.
async function onVisual(reader) {
  try {
    $("board").innerHTML = renderVisual(JSON.parse(await reader.readAll()));
    $("board").hidden = false;
    // The panel flashes when it redraws.
    $("board").classList.remove("redraw");
    void $("board").offsetWidth;
    $("board").classList.add("redraw");
  } catch {
    // A description this page cannot draw is skipped; the voice carries on.
  }
}

function watchTutor(participant) {
  if (participant.kind !== ParticipantKind.AGENT) return;
  clearTimeout(tutorTimer);
  tutorIdentity = participant.identity;
  const state = participant.attributes[AGENT_STATE] ?? "initializing";
  setTutorState(state);
  const manual = participant.attributes[TURNS] === "manual";
  const ready = state !== "initializing";
  if (manual === manualTurns && ready === tutorReady) return;
  manualTurns = manual;
  tutorReady = ready;
  talking = false;
  showTalk();
  syncMicrophone();
}

// While the connection is down the tutor neither hears nor is heard, and the
// last state it reported would be shown as if it were still true.
function showConnection(lost) {
  if (lost) return setTutorState("reconnecting");
  const tutor = [...(room?.remoteParticipants.values() ?? [])].find(
    (participant) => participant.kind === ParticipantKind.AGENT,
  );
  if (tutor) watchTutor(tutor);
  else setTutorState("left");
}

// Open only when the tutor is there and turns end by themselves; with a talk
// button the microphone is closed between turns.
function syncMicrophone() {
  return room?.localParticipant.setMicrophoneEnabled(wanted.mic && tutorReady && !manualTurns);
}

// The switch is the device's mute: while it is off nothing is heard, held
// button or not.
function showTalk() {
  $("talk").hidden = !manualTurns;
  $("talk").disabled = !wanted.mic;
  $("talk").setAttribute("aria-pressed", String(talking));
  $("talk").textContent = !wanted.mic
    ? "Microphone is off"
    : talking
      ? "Listening… let go to send"
      : "Hold to talk (or hold the space bar)";
}

function turn(method) {
  return room.localParticipant.performRpc({ destinationIdentity: tutorIdentity, method, payload: "" });
}

async function startTalking() {
  if (!room || !manualTurns || talking || !wanted.mic || !tutorIdentity) return;
  talking = true;
  showTalk();
  try {
    await room.localParticipant.setMicrophoneEnabled(true);
    await turn("start_turn");
  } catch {
    await stopTalking("cancel_turn");
    showMessage("The tutor could not be told that you are talking. Try again.");
  }
}

async function stopTalking(method = "end_turn") {
  if (!talking) return;
  talking = false;
  showTalk();
  const speaking = room;
  if (method === "end_turn") await new Promise((done) => setTimeout(done, TALK_TAIL_MS));
  if (!speaking || speaking !== room) return;
  try {
    await turn(method);
  } catch {
    if (method === "end_turn") showMessage("The tutor did not get that. Hold the button and say it again.");
  }
  if (!talking) await speaking.localParticipant.setMicrophoneEnabled(false);
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
    .on(RoomEvent.Reconnecting, () => showConnection(true))
    .on(RoomEvent.SignalReconnecting, () => showConnection(true))
    .on(RoomEvent.Reconnected, () => showConnection(false))
    .on(RoomEvent.Disconnected, (reason) =>
      leave(
        LOST.has(reason)
          ? "The connection was lost and the session could not be resumed. Start a new one."
          : "The session ended.",
      ),
    );
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
  sessionId = session.session_id ?? null;
  clearTimeout(reportTimer);
  $("report").hidden = true;
  if (session.session_id && $("operator").checked) {
    showSupervisor(session.session_id);
    supervisorTimer = setInterval(() => showSupervisor(session.session_id), SUPERVISOR_POLL_MS);
  }
  lobby.hidden = true;
  stage.hidden = false;

  await syncMicrophone();
  setToggle($("mic"), "Microphone", wanted.mic);
  showTalk();
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
  tutorIdentity = null;
  tutorReady = false;
  manualTurns = false;
  talking = false;
  showTalk();
  leaving.disconnect();
  $("audio").replaceChildren();
  $("board").replaceChildren();
  $("board").hidden = true;
  stage.hidden = true;
  lobby.hidden = true;
  entry.hidden = false;
  showMessage(reason ?? "");
  if (sessionId && $("operator").checked) awaitReport(sessionId);
  sessionId = null;
}

$("entry").addEventListener("submit", (event) => {
  event.preventDefault();
  showMessage("");
  openLobby();
});

// A signed-in person who has chosen a student goes to the same lobby.
onEnter(() => {
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
      body: JSON.stringify(sessionRequest({ code: $("code").value, language: $("language").value })),
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
  wanted.mic = !wanted.mic;
  setToggle($("mic"), "Microphone", wanted.mic);
  if (manualTurns) {
    if (!wanted.mic) await stopTalking("cancel_turn");
    showTalk();
  } else {
    await syncMicrophone();
  }
});

$("talk").addEventListener("pointerdown", (event) => {
  event.preventDefault();
  $("talk").setPointerCapture(event.pointerId);
  startTalking();
});
$("talk").addEventListener("pointerup", () => stopTalking());
$("talk").addEventListener("pointercancel", () => stopTalking("cancel_turn"));
$("talk").addEventListener("contextmenu", (event) => event.preventDefault());

// The space bar is the button too, except while typing or on another control.
const typing = (target) => ["INPUT", "SELECT", "TEXTAREA", "BUTTON"].includes(target?.tagName);
document.addEventListener("keydown", (event) => {
  if (event.code !== "Space" || event.repeat || !manualTurns || stage.hidden || typing(event.target)) return;
  event.preventDefault();
  startTalking();
});
document.addEventListener("keyup", (event) => {
  if (event.code === "Space" && talking) stopTalking();
});
// A turn nobody is holding any more is dropped, not sent.
window.addEventListener("blur", () => stopTalking("cancel_turn"));

$("device").addEventListener("click", () => {
  const on = !$("board").classList.contains("device");
  $("board").classList.toggle("device", on);
  setToggle($("device"), "Device screen", on);
});

$("cam").addEventListener("click", async () => {
  await room.localParticipant.setCameraEnabled(!room.localParticipant.isCameraEnabled);
  showCamera();
});

$("type").addEventListener("submit", async (event) => {
  event.preventDefault();
  const text = $("typed").value.trim();
  if (!text || !room) return;
  $("typed").value = "";
  try {
    await room.localParticipant.sendText(text, { topic: CHAT_TOPIC });
    // Typed words are not transcribed back, so the page writes them itself.
    const line = item(text);
    line.dataset.who = "you";
    line.dataset.name = "You (typed)";
    $("transcript").append(line);
    $("transcript").scrollTop = $("transcript").scrollHeight;
  } catch {
    showMessage("The message could not be sent.");
  }
});

$("end").addEventListener("click", () => leave());
