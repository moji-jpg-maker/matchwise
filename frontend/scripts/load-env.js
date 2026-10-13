#!/usr/bin/env node
/**
 * Load the repo-root `.env` into process.env for the local Tauri scripts.
 *
 * The Rust side reads these values with `std::env::var` and does not use a
 * dotenv crate, so something has to put them into the environment first.
 * `.env` deliberately lives at the repo root, next to the Cargo workspace,
 * rather than in `frontend/`: keeping it out of the Next.js directory stops
 * Next from ever inlining these values into the exported bundle.
 *
 * A variable that is already set in the real environment is never overwritten.
 * A missing file is not an error: the OS credential store is the default
 * source for the database key, the Telegram token and the AI key.
 */

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const ROOT_DIR = path.resolve(__dirname, '..', '..');
const ENV_PATH = path.join(ROOT_DIR, '.env');

function parse(text) {
  const vars = {};
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#')) continue;

    const eq = line.indexOf('=');
    if (eq <= 0) continue;

    const key = line.slice(0, eq).trim();
    let value = line.slice(eq + 1).trim();
    if (
      value.length >= 2 &&
      ((value.startsWith('"') && value.endsWith('"')) ||
        (value.startsWith("'") && value.endsWith("'")))
    ) {
      value = value.slice(1, -1);
    }
    vars[key] = value;
  }
  return vars;
}

/** Read `file` and fill in any variable that is not already set. Returns the count loaded. */
function loadEnvFile(file = ENV_PATH) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (err) {
    if (err.code === 'ENOENT') return 0;
    throw err;
  }

  let loaded = 0;
  for (const [key, value] of Object.entries(parse(text))) {
    if (process.env[key] !== undefined) continue;
    process.env[key] = value;
    loaded++;
  }
  return loaded;
}

module.exports = { loadEnvFile, parse, ENV_PATH };

if (require.main === module) {
  const loaded = loadEnvFile();
  if (loaded > 0) {
    console.log(`🔑 Loaded ${loaded} environment variable(s) from ${ENV_PATH}`);
  }

  // `tauri` is not a real executable: it is a JS file behind a shim in
  // node_modules/.bin. npm/pnpm put that directory on PATH only for the
  // duration of a package script, and on Windows the shim is a .cmd that
  // cannot be spawned without a shell. So resolve the CLI's JS entry directly
  // and run it with the current node binary: identical on every platform, and
  // arguments are passed through verbatim instead of being re-parsed by a
  // shell (`--features` and paths with spaces both survive).
  const [cmd, ...args] = process.argv.slice(2);
  if (cmd) {
    const env = { ...process.env };

    const binDir = path.join(__dirname, '..', 'node_modules', '.bin');
    if (fs.existsSync(binDir)) {
      env.PATH = `${binDir}${path.delimiter}${env.PATH ?? ''}`;
    }

    let file = cmd;
    let argv = args;
    if (cmd === 'tauri') {
      const cli = [
        path.join(__dirname, '..', 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
        path.join(__dirname, '..', '..', 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
      ].find((candidate) => fs.existsSync(candidate));
      if (cli) {
        file = process.execPath;
        argv = [cli, ...args];
      }
    }

    const result = spawnSync(file, argv, { stdio: 'inherit', shell: false, env });
    if (result.error) {
      console.error(`Failed to run ${cmd}: ${result.error.message}`);
      process.exit(1);
    }
    process.exit(result.status === null ? 1 : result.status);
  }
}
