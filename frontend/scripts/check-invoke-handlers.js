#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const libPath = path.join(root, 'src-tauri', 'src', 'lib.rs');
const srcDir = path.join(root, 'src');

function walk(dir, out) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else if (/\.(ts|tsx)$/.test(entry.name)) out.push(full);
  }
  return out;
}

function registeredHandlers() {
  const source = fs.readFileSync(libPath, 'utf8');
  const start = source.indexOf('generate_handler![');
  if (start === -1) throw new Error('generate_handler! not found in lib.rs');
  const open = start + 'generate_handler!['.length;
  let depth = 1;
  let end = open;
  while (end < source.length && depth > 0) {
    const ch = source[end];
    if (ch === '[') depth += 1;
    else if (ch === ']') depth -= 1;
    end += 1;
  }
  if (depth !== 0) throw new Error('unterminated generate_handler! list in lib.rs');
  const body = source
    .slice(open, end - 1)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/.*$/gm, '')
    .replace(/#\[[^\]]*(?:\[[^\]]*\][^\]]*)*\]/g, '');
  const names = new Set();
  for (const item of body.split(',')) {
    const trimmed = item.trim();
    if (!trimmed) continue;
    names.add(trimmed.split('::').pop());
  }
  return names;
}

function invokedCommands() {
  const pattern = /\binvoke(?:Tauri)?\s*(?:<[^()]*?>)?\s*\(\s*['"`]([A-Za-z0-9_]+)['"`]/g;
  const found = new Map();
  for (const file of walk(srcDir, [])) {
    const text = fs.readFileSync(file, 'utf8');
    let match;
    while ((match = pattern.exec(text)) !== null) {
      const line = text.slice(0, match.index).split('\n').length;
      if (!found.has(match[1])) found.set(match[1], []);
      found.get(match[1]).push(`${path.relative(root, file)}:${line}`);
    }
  }
  return found;
}

const handlers = registeredHandlers();
const invoked = invokedCommands();

const missing = [...invoked.keys()].filter((name) => !handlers.has(name)).sort();
const unused = [...handlers].filter((name) => !invoked.has(name)).sort();

if (process.argv.includes('--list-unused')) {
  console.log(`Registered but never invoked with a literal name (${unused.length}):`);
  for (const name of unused) console.log(`  ${name}`);
}

if (missing.length > 0) {
  console.error(`invoke() calls with no handler in generate_handler! (${missing.length}):`);
  for (const name of missing) console.error(`  ${name}  <- ${invoked.get(name).join(', ')}`);
  process.exit(1);
}

console.log(`OK: ${invoked.size} invoked commands all registered (${handlers.size} handlers, ${unused.length} not invoked by literal name).`);
