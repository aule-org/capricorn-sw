# Contracts

The events Aule's components send each other, as JSON Schema. This folder is the single source of truth: the Node platform and the Python agent both validate against these files, so a change here that one side does not follow fails its tests.

```text
envelope.schema.json   the envelope every event uses (CloudEvents 1.0, structured JSON)
events/                one schema per event type and version, for the envelope's `data`
examples/              one valid example per event type, used by the tests of both sides
visuals/               what the tutor shows on a screen: one schema and an example per kind
```

## Rules

- **Names:** `<thing>.<what_happened>.v<N>`, past tense: `session.ended.v1`.
- **Versions:** adding an optional field keeps the version. Removing or renaming a field, changing a type or making a field required needs a new version (`v2`) next to the old one, until no consumer reads the old one.
- **No unknown fields:** every event schema sets `additionalProperties: false`.
- **Privacy:** events never carry a child's words, audio, transcripts or model scores. They carry IDs, timestamps, counts and fixed categories. If a consumer needs more, it asks the platform through an authenticated API.
- **Every event type has an example** in `examples/`, as a full envelope.

## Check

```bash
pnpm install
pnpm test                               # Node: schemas compile, examples validate
cd apps/agent && uv run pytest          # Python: same examples, plus the agent's event builder
```


## Visuals

`visuals/visual.v1.schema.json` describes what the tutor can put on the student's screen: the statement of an exercise, fraction bars, a number line, an array of dots, a table, a bar chart or a rectangle with its sides. It is a description, not a picture. The agent's `show` tool takes exactly this shape, checks it and sends it to the room on the text-stream topic `aule.visual`; each screen draws it itself, in black on white, so it costs nothing to render and works on e-ink. A bar or a circle has its fraction written next to it unless the fraction says `labelled: false`, for when the fraction is what the student has to find. A new kind needs its example here, a drawing in every screen that shows visuals, and a line in the tool's description.
