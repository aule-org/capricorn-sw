// Every catalog in this directory must be loadable by PUT /v1/skills and form
// a prerequisite graph the learner model can walk: no dangling ids, no cycles,
// nothing that depends on a later grade.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { test } from 'node:test';

const root = new URL('../', import.meta.url);
const files = readdirSync(root).filter((file) => file.endsWith('.json') && file !== 'package.json');

// Same rule as is_slug in apps/platform/src/learning/mod.rs.
const SLUG = /^[a-z0-9][a-z0-9._-]{0,79}$/;
const GRADES = ['P1', 'P2', 'P3', 'P4', 'P5', 'P6'];
const FIELDS = ['id', 'subject', 'name', 'grade', 'description', 'prerequisites'];
const MAX_NAME = 160;
const MAX_DESCRIPTION = 600;
const MAX_SKILLS_PER_REQUEST = 2000;

test('there is at least one catalog', () => {
  assert.ok(files.length > 0);
});

for (const file of files) {
  const load = () => JSON.parse(readFileSync(new URL(file, root), 'utf8'));

  test(`${file}: is a JSON object with a skills array that fits one request`, () => {
    const catalog = load();
    assert.deepEqual(Object.keys(catalog), ['skills']);
    assert.ok(Array.isArray(catalog.skills));
    assert.ok(catalog.skills.length > 0);
    assert.ok(catalog.skills.length <= MAX_SKILLS_PER_REQUEST);
  });

  test(`${file}: ids are unique slugs`, () => {
    const seen = new Set();
    for (const { id } of load().skills) {
      assert.equal(typeof id, 'string');
      assert.match(id, SLUG);
      assert.ok(!seen.has(id), `duplicate id ${id}`);
      seen.add(id);
    }
  });

  test(`${file}: every skill has the required fields within their limits`, () => {
    for (const skill of load().skills) {
      assert.deepEqual(Object.keys(skill).sort(), [...FIELDS].sort(), `${skill.id}: unexpected or missing fields`);
      assert.equal(skill.subject, 'math', `${skill.id}: subject`);
      assert.ok(GRADES.includes(skill.grade), `${skill.id}: grade ${skill.grade}`);
      for (const [field, max] of [['name', MAX_NAME], ['description', MAX_DESCRIPTION]]) {
        const value = skill[field];
        assert.equal(typeof value, 'string', `${skill.id}: ${field} must be a string`);
        assert.ok(value.trim().length > 0, `${skill.id}: ${field} is empty`);
        assert.equal(value, value.trim(), `${skill.id}: ${field} has surrounding whitespace`);
        assert.ok([...value].length <= max, `${skill.id}: ${field} is longer than ${max} characters`);
      }
      assert.ok(Array.isArray(skill.prerequisites), `${skill.id}: prerequisites must be an array`);
    }
  });

  test(`${file}: prerequisites exist, are not repeated and never come from a later grade`, () => {
    const { skills } = load();
    const byId = new Map(skills.map((skill) => [skill.id, skill]));
    for (const skill of skills) {
      assert.equal(new Set(skill.prerequisites).size, skill.prerequisites.length, `${skill.id}: repeated prerequisite`);
      for (const id of skill.prerequisites) {
        assert.notEqual(id, skill.id, `${skill.id} lists itself`);
        const prerequisite = byId.get(id);
        assert.ok(prerequisite, `${skill.id}: unknown prerequisite ${id}`);
        assert.ok(
          GRADES.indexOf(prerequisite.grade) <= GRADES.indexOf(skill.grade),
          `${skill.id} (${skill.grade}) depends on ${id} (${prerequisite.grade})`,
        );
      }
    }
  });

  test(`${file}: the prerequisite graph has no cycles`, () => {
    const { skills } = load();
    const byId = new Map(skills.map((skill) => [skill.id, skill]));
    const state = new Map(); // id -> 'visiting' | 'done'
    const visit = (id, path) => {
      if (state.get(id) === 'done' || !byId.has(id)) return;
      assert.notEqual(state.get(id), 'visiting', `cycle: ${[...path, id].join(' -> ')}`);
      state.set(id, 'visiting');
      for (const next of byId.get(id).prerequisites) visit(next, [...path, id]);
      state.set(id, 'done');
    };
    for (const { id } of skills) visit(id, []);
  });

  test(`${file}: every grade has enough skills and P1 has places to start`, () => {
    const { skills } = load();
    for (const grade of GRADES) {
      const count = skills.filter((skill) => skill.grade === grade).length;
      assert.ok(count >= 15, `${grade} has only ${count} skills`);
    }
    const entry = skills.filter((skill) => skill.grade === 'P1' && skill.prerequisites.length === 0);
    assert.ok(entry.length >= 5, `P1 has only ${entry.length} skills without prerequisites`);
  });
}
