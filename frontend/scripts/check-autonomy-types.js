#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const types = fs.readFileSync(path.join(root, 'src', 'types', 'matchmaking.ts'), 'utf8');
const fixture = JSON.parse(
  fs.readFileSync(path.join(root, '..', 'matchmaking-core', 'tests', 'fixtures', 'autonomy', 'policy.json'), 'utf8'),
);

function block(pattern, label) {
  const match = types.match(pattern);
  if (!match) {
    console.error(`autonomy types: could not find ${label} in types/matchmaking.ts`);
    process.exit(1);
  }
  return match[1];
}

function quoted(text) {
  return [...text.matchAll(/'([a-z0-9_]+)'/g)].map((match) => match[1]);
}

const unionSteps = quoted(block(/export type AutonomyStep =([^;]*);/, 'AutonomyStep'));
const listedSteps = quoted(block(/export const AUTONOMY_STEPS: AutonomyStep\[\] = \[([^\]]*)\]/, 'AUTONOMY_STEPS'));
const unionLevels = quoted(block(/export type AutonomyLevel =([^;]*);/, 'AutonomyLevel'));
const listedLevels = quoted(block(/export const AUTONOMY_LEVELS: AutonomyLevel\[\] = \[([^\]]*)\]/, 'AUTONOMY_LEVELS'));
const modes = quoted(block(/export const AUTONOMY_MODE_NAMES = \[([^\]]*)\]/, 'AUTONOMY_MODE_NAMES'));
const modeUnion = block(/export type AutonomyStepMode =([^;]*);/, 'AutonomyStepMode');
const builtSteps = quoted(block(/export const BUILT_AUTONOMY_STEPS: AutonomyStep\[\] = \[([^\]]*)\]/, 'BUILT_AUTONOMY_STEPS'));

const problems = [];

function same(label, actual, expected) {
  const missing = expected.filter((name) => !actual.includes(name));
  const extra = actual.filter((name) => !expected.includes(name));
  if (missing.length) problems.push(`${label}: missing ${missing.join(', ')}`);
  if (extra.length) problems.push(`${label}: not in the policy fixture ${extra.join(', ')}`);
}

const fixtureSteps = Object.keys(fixture.steps);
same('AutonomyStep', unionSteps, fixtureSteps);
same('AUTONOMY_STEPS', listedSteps, fixtureSteps);
same('AutonomyLevel', unionLevels, [fixture.level, ...unionLevels]);
same('AUTONOMY_LEVELS', listedLevels, unionLevels);
if (!unionLevels.includes(fixture.level)) problems.push(`AutonomyLevel: fixture level ${fixture.level} is not declared`);

const fixtureModes = new Set(
  Object.values(fixture.steps).map((mode) => (typeof mode === 'string' ? mode : Object.keys(mode)[0])),
);
for (const mode of fixtureModes) {
  if (!modes.includes(mode)) problems.push(`AUTONOMY_MODE_NAMES: fixture uses ${mode}`);
  if (!modeUnion.includes(mode)) problems.push(`AutonomyStepMode: fixture uses ${mode}`);
}
for (const mode of modes) {
  if (!modeUnion.includes(mode)) problems.push(`AutonomyStepMode: ${mode} is not declared`);
}
for (const step of builtSteps) {
  if (!fixtureSteps.includes(step)) problems.push(`BUILT_AUTONOMY_STEPS: unknown step ${step}`);
}
for (const key of ['version', 'level', 'paused', 'min_confidence', 'steps']) {
  if (!(key in fixture)) problems.push(`fixture lost the ${key} key`);
  if (!new RegExp(`\\b${key}:`).test(block(/export interface AutonomyPolicy \{([^}]*)\}/, 'AutonomyPolicy'))) {
    problems.push(`AutonomyPolicy: missing ${key}`);
  }
}

if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log(`autonomy types: ${fixtureSteps.length} steps, ${listedLevels.length} levels, ${modes.length} modes match the core fixture`);
