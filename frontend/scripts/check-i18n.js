#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

const dir = path.join(__dirname, '..', 'src', 'i18n');

const unparsed = [];

function flatten(source) {
  const out = {};
  const pattern = /^\s*([A-Za-z0-9_]+):\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)"|`((?:[^`\\]|\\.)*)`),?\s*$/;
  const stack = [];
  for (const line of source.split('\n')) {
    const open = line.match(/^\s*([A-Za-z0-9_]+):\s*\{\s*$/);
    if (open) {
      stack.push(open[1]);
      continue;
    }
    if (/^\s*\},?\s*$/.test(line) && stack.length) {
      stack.pop();
      continue;
    }
    const entry = line.match(pattern);
    if (entry) {
      out[[...stack, entry[1]].join('.')] = entry[2] ?? entry[3] ?? entry[4];
      continue;
    }
    if (/^\s*[A-Za-z0-9_]+:\s*(?:['"`].*)?$/.test(line)) unparsed.push(line.trim());
  }
  return out;
}

function params(text) {
  return [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort().join(',');
}

const en = flatten(fs.readFileSync(path.join(dir, 'en.ts'), 'utf8'));
const fa = flatten(fs.readFileSync(path.join(dir, 'fa.ts'), 'utf8'));
const problems = [];

for (const key of Object.keys(en)) {
  if (!(key in fa)) problems.push(`missing in fa: ${key}`);
  else if (params(en[key]) !== params(fa[key])) problems.push(`param mismatch: ${key}`);
}
for (const key of Object.keys(fa)) {
  if (!(key in en)) problems.push(`extra in fa: ${key}`);
}

for (const line of unparsed) problems.push(`value not on one line: ${line.slice(0, 60)}`);

const srcDir = path.join(__dirname, '..', 'src');
const hardcoded = [];
const ALLOWED = /^(Matchwise|Claude|Groq|Ollama|OpenAI|OpenRouter|https?:\/\/\S+)$/;

function walk(folder) {
  for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
    const full = path.join(folder, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'i18n' && entry.name !== 'node_modules') walk(full);
    } else if (entry.name.endsWith('.tsx') && !entry.name.includes('.test.') && !entry.name.endsWith('Test.tsx')) {
      const lines = fs.readFileSync(full, 'utf8').split('\n');
      let inComment = false;
      lines.forEach((line, index) => {
        const wasInComment = inComment;
        const opens = (line.match(/\/\*/g) || []).length;
        const closes = (line.match(/\*\//g) || []).length;
        if (opens > closes) inComment = true;
        else if (closes > opens || (closes && wasInComment)) inComment = false;
        if (wasInComment || opens || closes) return;
        const text = line.match(/(?<!=)>\s*([A-Za-z][A-Za-z ,.'!?:-]{3,})\s*<\/?[A-Za-z]/);
        const attr = line.match(/\b(?:placeholder|title|aria-label|alt)="([A-Za-z][^"{}]{3,})"/);
        const found = text?.[1] ?? attr?.[1];
        if (found && !ALLOWED.test(found.trim()) && !/^\s*(\/\/|\*|import )/.test(line)) {
          hardcoded.push(`${path.relative(srcDir, full)}:${index + 1}: ${found.trim()}`);
        }
      });
    }
  }
}

walk(srcDir);

if (hardcoded.length) {
  const strict = process.argv.includes('--strict');
  const report = `${hardcoded.length} possible hardcoded UI strings:\n${hardcoded.join('\n')}`;
  if (strict) problems.push(report);
  else console.warn(report);
}

if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log(`i18n catalogs in sync (${Object.keys(en).length} keys)`);
