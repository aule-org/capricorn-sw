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
const TURNS = "aule.turns";
// Set by the tutor when it can be shown a piece of work on request. The camera
// is then for one picture at a time, not for watching.
const SHOW_WORK = "aule.show_work";
// The picture is taken from the first frame after the request; the camera
// stays on a moment longer so the student sees what was shown.
const SHOWN_MS = 1500;
// How long the microphone stays open after the button is let go, so the last
// syllable is not cut.
const TALK_TAIL_MS = 200;
const TUTOR_WAIT_MS = 15_000;

const STATE_TEXT = {
  waiting: "Esperando a Aule…",
  initializing: "Aule se está preparando…",
  listening: "Te escucho",
  thinking: "Pensando…",
  speaking: "Hablando",
  missing:
    "Aule no pudo entrar a la sesión. Inténtalo otra vez en un momento.",
  left: "Aule salió de la sesión.",
  reconnecting: "Se perdió la conexión. Reconectando…",
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
// How often the page asks where an engine-run session stands. The tutor's own
// words arrive by voice; this only keeps the strip and the card in step.
const STEP_POLL_MS = 3000;
// The review is written after the session ends; it usually takes under a
// minute. After this long the page stops asking.
const REPORT_WAIT_MS = 120_000;
// Typed messages go to the tutor on the topic LiveKit agents read.
const CHAT_TOPIC = "lk.chat";

const ERRORS = {
  wrong_code: "Ese código de acceso no es correcto.",
  too_many_attempts: "Demasiados códigos equivocados. Espera un minuto e inténtalo otra vez.",
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
// The tutor takes a picture when asked, so the camera is off between pictures.
let photos = false;
let showing = false;
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
let stepTimer = null;
// What the card last showed, so a poll that changes nothing leaves a
// half-written answer alone.
let cardShown = "";
let reportTimer = null;
// The platform's id for the session being shown, when there is one.
let sessionId = null;

function showMessage(text) {
  message.textContent = text;
  message.hidden = !text;
}

// For something that went wrong once and can be tried again: it does not stay
// on the screen for the rest of the session.
const PASSING_MS = 8000;
function showPassingMessage(text) {
  showMessage(text);
  setTimeout(() => {
    if (message.textContent === text) showMessage("");
  }, PASSING_MS);
}

// What Aule's face does in each state of the session.
const MASCOT_STATE = {
  waiting: "sleeping",
  initializing: "thinking",
  listening: "listening",
  thinking: "thinking",
  speaking: "speaking",
  missing: "sad",
  left: "sleeping",
  reconnecting: "surprised",
};

// The face takes the dot's place once its script has loaded; in a browser
// that cannot run it, the dot stays.
customElements.whenDefined("wobnimo-mascot").then(() => $("tutor").classList.add("has-mascot"));

function setTutorState(state) {
  $("tutor").dataset.state = state;
  $("tutor-state").textContent = STATE_TEXT[state] ?? state;
  $("mascot").setAttribute("state", MASCOT_STATE[state] ?? "idle");
}

function setToggle(button, label, on) {
  button.setAttribute("aria-pressed", String(on));
  // "Cámara apagada", "Micrófono apagado": the word follows the thing's gender.
  const off = label.split(" ")[0].endsWith("a") ? "apagada" : "apagado";
  button.textContent = on ? label : `${label} ${off}`;
}

function startClock(seconds) {
  const endsAt = Date.now() + seconds * 1000;
  const tick = () => {
    const left = Math.max(0, Math.round((endsAt - Date.now()) / 1000));
    $("clock").textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
    if (left === 0) leave("Se acabó el tiempo de la sesión.");
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
    line.dataset.name = mine ? "Tú" : "Aule";
    lines.set(id, line);
    $("transcript").append(line);
    followTranscript();
  }
  // The person's speech arrives as whole revisions; the tutor's as increments.
  let text = "";
  for await (const chunk of reader) {
    text += chunk;
    line.textContent = text;
    followTranscript();
  }
}

// The page stays where the person left it; only the conversation's own column
// follows the newest words. It stops following when the person scrolls up to
// read something earlier, and starts again when they come back to the end.
let following = true;
let followFrame = 0;

$("transcript").addEventListener("scroll", () => {
  const list = $("transcript");
  following = list.scrollHeight - list.scrollTop - list.clientHeight < 48;
});

function followTranscript(always = false) {
  if (always) following = true;
  if (!following || followFrame) return;
  // Once per frame, after the new words have their height.
  followFrame = requestAnimationFrame(() => {
    followFrame = 0;
    const list = $("transcript");
    list.scrollTop = list.scrollHeight;
  });
}

// --- a session run by the engine: its stages and the exercise in hand ---------

const ROUTE_STATE = { done: "Hecho", current: "Ahora", pending: "Después" };

function stopSteps() {
  clearInterval(stepTimer);
  stepTimer = null;
  cardShown = "";
  $("route").hidden = true;
  $("card").hidden = true;
  document.querySelector(".work").classList.remove("with-card");
}

function followSteps(id) {
  stopSteps();
  const ask = () => showStep(id).catch(() => undefined);
  ask();
  stepTimer = setInterval(ask, STEP_POLL_MS);
}

async function engineCall(path, id, more = {}) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    // The code only matters for the team's sessions; a family is known by its sign-in.
    body: JSON.stringify({ code: $("code").value, session_id: id, ...more }),
  });
  if (response.status === 409) return null;
  if (!response.ok) throw new Error("step");
  return response.json();
}

async function showStep(id, given) {
  const step = given ?? (await engineCall("api/step", id));
  if (id !== sessionId) return;
  // Not the engine's session, or it is over: nothing more to follow.
  if (!step) return stopSteps();

  $("route").replaceChildren(
    ...(step.route ?? []).map((stop) => {
      const line = document.createElement("li");
      line.dataset.state = stop.state;
      const name = document.createElement("strong");
      name.textContent = stop.name;
      const detail = document.createElement("span");
      detail.textContent = stop.detail ?? "";
      line.append(name, detail);
      line.setAttribute("aria-label", `${stop.name}: ${ROUTE_STATE[stop.state] ?? ""}`);
      if (stop.state === "current") line.setAttribute("aria-current", "step");
      return line;
    }),
  );
  $("route").hidden = !(step.route ?? []).length;

  const card = step.card;
  const shown = card ? `${card.title}|${card.statement}` : "";
  if (shown === cardShown) return;
  cardShown = shown;
  $("card").hidden = !card;
  document.querySelector(".work").classList.toggle("with-card", Boolean(card));
  if (!card) return;
  $("card-title").textContent = card.title ?? "";
  $("card-statement").textContent = card.statement ?? "";
  $("answer").hidden = !card.asks_answer;
  $("explanation-line").hidden = !card.asks_explanation;
  $("card-help").textContent = card.format_help ?? "";
  $("card-help").hidden = !card.format_help;
  $("answer-text").value = "";
  $("explanation-text").value = "";
  // A new exercise brings its own drawing, or an empty board: the drawing of
  // the one before does not stay next to it.
  $("board").hidden = true;
  if (card.visual && card.visual.kind !== "exercise") {
    try {
      $("board").innerHTML = renderVisual(card.visual);
      $("board").hidden = false;
    } catch {
      // A drawing the page cannot make is simply not shown.
    }
  }
}

$("answer").addEventListener("submit", async (event) => {
  event.preventDefault();
  const id = sessionId;
  const written = $("answer-text").value.trim();
  if (!id || !written) return;
  $("answer-send").disabled = true;
  showMessage("");
  try {
    const explanation = $("explanation-text").value.trim();
    const next = await engineCall("api/answer", id, explanation ? { answer: written, explanation } : { answer: written });
    // What they wrote is part of the conversation too.
    const line = item(explanation ? `${written}. ${explanation}` : written);
    line.dataset.who = "you";
    line.dataset.name = "Tú (escrito)";
    $("transcript").append(line);
    followTranscript(true);
    // The tutor says what the engine answered: tell it there is something waiting.
    if (room && tutorIdentity) {
      room.localParticipant
        .performRpc({ destinationIdentity: tutorIdentity, method: "answered", payload: "" })
        .catch(() => undefined);
    }
    cardShown = "";
    await showStep(id, next ?? undefined);
  } catch {
    showMessage("No se pudo enviar tu respuesta. Inténtalo otra vez o dila en voz alta.");
  } finally {
    $("answer-send").disabled = false;
  }
});

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
    $("plan").textContent = view.brief?.goal ?? "No hay plan para esta sesión.";
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
            `${exercise.probe ? "Diagnóstico" : "Práctica"}: ${exercise.expression} = ${exercise.answer}`,
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
  $("report-note").textContent = "Se está revisando la sesión…";
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
      $("report-note").textContent = "El resumen de arriba es lo que lee la familia. El plan y la conversación no se envían.";
    } else if (Date.now() - started < REPORT_WAIT_MS) {
      reportTimer = setTimeout(ask, SUPERVISOR_POLL_MS);
    } else {
      $("report-note").textContent = "No se escribió un informe para esta sesión.";
    }
  };
  ask();
}

// The tutor describes what to show; the page draws it.
async function onVisual(reader) {
  try {
    const visual = JSON.parse(await reader.readAll());
    // With the exercise card on the screen its statement is already there:
    // the board is for drawings, not for the same words again.
    if (visual.kind === "exercise" && !$("card").hidden) return;
    $("board").innerHTML = renderVisual(visual);
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
  usePhotos(participant.attributes[SHOW_WORK] === "rpc");
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

// A phone has no space bar to mention.
const touchOnly = matchMedia("(pointer: coarse)").matches && !matchMedia("(pointer: fine)").matches;

// The switch is the device's mute: while it is off nothing is heard, held
// button or not.
function showTalk() {
  $("talk").hidden = !manualTurns;
  // On a phone the button then stays at the bottom of the screen (style.css).
  document.querySelector("main").classList.toggle("push-to-talk", manualTurns);
  $("talk").disabled = !wanted.mic;
  $("talk").setAttribute("aria-pressed", String(talking));
  $("talk").textContent = !wanted.mic
    ? "El micrófono está apagado"
    : talking
      ? "Te escucho… suelta para enviar"
      : touchOnly
        ? "Mantén apretado para hablar"
        : "Mantén apretado para hablar (o la barra espaciadora)";
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
    showMessage("No se pudo avisar a Aule que estás hablando. Inténtalo otra vez.");
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
    if (method === "end_turn") showMessage("Aule no lo escuchó. Mantén apretado el botón y dilo otra vez.");
  }
  if (!talking) await speaking.localParticipant.setMicrophoneEnabled(false);
}

function showCamera() {
  const publication = room.localParticipant.getTrackPublication(Track.Source.Camera);
  const on = Boolean(publication?.track) && !publication.isMuted;
  if (on) publication.track.attach($("camera"));
  $("camera").hidden = !on;
  $("camera-off").hidden = on;
  $("camera-off").textContent = photos ? "La cámara se enciende solo cuando muestras tu tarea" : "Cámara apagada";
  if (photos) {
    $("cam").setAttribute("aria-pressed", String(showing));
    $("cam").textContent = showing ? "Mostrando…" : "Mostrar mi tarea";
    $("cam").disabled = showing;
  } else {
    $("cam").disabled = false;
    setToggle($("cam"), "Cámara", on);
  }
}

// With a tutor that takes pictures, nothing is filmed between them.
function usePhotos(on) {
  if (on === photos || !room) return;
  photos = on;
  if (on && !showing) room.localParticipant.setCameraEnabled(false).then(showCamera, showCamera);
  else showCamera();
}

// One picture of the work: the camera goes on, the tutor is told to look, and
// the camera goes off again.
async function showWork() {
  if (showing || !room || !tutorIdentity) return;
  showing = true;
  showMessage("");
  try {
    await room.localParticipant.setCameraEnabled(true);
    showCamera();
    // The default timeout on purpose: the tutor waits up to two seconds for a
    // frame, and a shorter limit here would cut that wait.
    const seen = await room.localParticipant.performRpc({
      destinationIdentity: tutorIdentity,
      method: "show_work",
      payload: "",
    });
    if (seen !== "ok") throw new Error("no frame");
    // The tutor hears a fixed line for this; the page writes the same one.
    const line = item("Mira mi trabajo.");
    line.dataset.who = "you";
    line.dataset.name = "Tú";
    $("transcript").append(line);
    followTranscript(true);
    await new Promise((resolve) => setTimeout(resolve, SHOWN_MS));
  } catch {
    showPassingMessage("Aule no pudo ver la cámara. Acerca la hoja e inténtalo otra vez.");
  } finally {
    showing = false;
    await room?.localParticipant.setCameraEnabled(false).catch(() => undefined);
    if (room) showCamera();
  }
}

async function join(session) {
  room = new Room({
    adaptiveStream: true,
    dynacast: true,
    // At home there is a television and other people. The browser's own
    // clean-up costs nothing; voice isolation only acts where the system has it.
    audioCaptureDefaults: {
      echoCancellation: true,
      noiseSuppression: true,
      autoGainControl: true,
      voiceIsolation: true,
    },
    // The camera is for showing work: the one facing the table when there are
    // two, and sharp enough to read handwriting.
    videoCaptureDefaults: { facingMode: "environment", resolution: { width: 1280, height: 720 } },
  });

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
          ? "Se perdió la conexión y la sesión no pudo continuar. Empieza una nueva."
          : "La sesión terminó.",
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
  if (session.session_id && session.flow === "engine") followSteps(session.session_id);
  clearTimeout(reportTimer);
  $("report").hidden = true;
  if (session.session_id && $("operator").checked) {
    showSupervisor(session.session_id);
    supervisorTimer = setInterval(() => showSupervisor(session.session_id), SUPERVISOR_POLL_MS);
  }
  lobby.hidden = true;
  stage.hidden = false;
  // The layout of a session hangs on this class, not on :has(), which older
  // Firefox and Safari do not know.
  document.querySelector("main").classList.toggle("in-session", true);

  await syncMicrophone();
  setToggle($("mic"), "Micrófono", wanted.mic);
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
  setToggle($("lobby-mic"), "Micrófono", wanted.mic);
  setToggle($("lobby-cam"), "Cámara", wanted.cam);
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
    preview = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      video: { facingMode: { ideal: "environment" } },
    });
  } catch {
    // No camera, or only the camera was refused: try the microphone alone.
    wanted.cam = false;
    try {
      preview = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      $("lobby-note").textContent = "No se encontró o no se permitió la cámara. Igual puedes entrar con voz.";
    } catch {
      wanted.mic = false;
      $("lobby-note").textContent =
        "El navegador bloqueó el micrófono. Permítelo en la barra de direcciones y vuelve a intentarlo.";
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
  stopSteps();
  $("supervisor").hidden = true;
  tutorIdentity = null;
  photos = false;
  showing = false;
  tutorReady = false;
  manualTurns = false;
  talking = false;
  showTalk();
  leaving.disconnect();
  $("audio").replaceChildren();
  $("board").replaceChildren();
  $("board").hidden = true;
  stage.hidden = true;
  document.querySelector("main").classList.toggle("in-session", false);
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
  following = true;
  lines.clear();
  try {
    const response = await fetch("api/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(sessionRequest({ code: $("code").value, language: $("language").value })),
    });
    const body = await response.json();
    if (!response.ok) {
      throw new Error(ERRORS[body.error] ?? body.message ?? "No se pudo empezar la sesión.");
    }
    // The room opens the devices itself, with what was chosen here.
    stopPreview();
    await join(body);
  } catch (error) {
    const text =
      error?.name === "NotAllowedError"
        ? "El navegador bloqueó el micrófono. Permítelo y empieza otra vez."
        : (error?.message ?? "No se pudo empezar la sesión.");
    if (room) leave(text);
    else {
      closeLobby();
      showMessage(text);
    }
  }
});

$("mic").addEventListener("click", async () => {
  wanted.mic = !wanted.mic;
  setToggle($("mic"), "Micrófono", wanted.mic);
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
  setToggle($("device"), "Modo tinta", on);
});

$("cam").addEventListener("click", async () => {
  if (photos) return showWork();
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
    line.dataset.name = "Tú (escrito)";
    $("transcript").append(line);
    // Their own message: back to the end, wherever they had scrolled to.
    followTranscript(true);
  } catch {
    showMessage("No se pudo enviar el mensaje.");
  }
});

$("end").addEventListener("click", () => leave());
