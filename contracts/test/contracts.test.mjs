// Every schema must compile, every event type must have an example, and every
// example must validate against the envelope and its event schema.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const root = new URL('../', import.meta.url);
const read = (path) => JSON.parse(readFileSync(new URL(path, root), 'utf8'));

const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
const validateEnvelope = ajv.compile(read('envelope.schema.json'));

const types = readdirSync(new URL('events/', root))
  .filter((file) => file.endsWith('.schema.json'))
  .map((file) => file.replace('.schema.json', ''));

test('there is at least one event type', () => {
  assert.ok(types.length > 0);
});

for (const type of types) {
  test(type, () => {
    const schema = read(`events/${type}.schema.json`);
    assert.equal(schema.title, type, 'schema title must be the event type');
    assert.equal(schema.additionalProperties, false, 'event data must not allow unknown fields');
    const validateData = ajv.compile(schema);

    const example = read(`examples/${type}.json`);
    assert.equal(example.type, type);
    assert.ok(validateEnvelope(example), JSON.stringify(validateEnvelope.errors));
    assert.ok(validateData(example.data), JSON.stringify(validateData.errors));

    assert.equal(validateData({ ...example.data, unexpected: true }), false, 'unknown fields must be rejected');
  });
}

test('every example has a schema', () => {
  const examples = readdirSync(new URL('examples/', root)).map((file) => file.replace('.json', ''));
  assert.deepEqual(examples.sort(), [...types].sort());
});
