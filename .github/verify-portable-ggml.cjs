const { existsSync, readdirSync, readFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const PORTABILITY_FLAGS = [
  'GGML_NATIVE',
  'GGML_AVX512',
  'GGML_AVX512_VBMI',
  'GGML_AVX512_VNNI',
  'GGML_AVX512_BF16',
];

function findWhisperCaches(buildDirectory) {
  if (!existsSync(buildDirectory)) {
    return [];
  }

  const caches = new Set();
  for (const entry of readdirSync(buildDirectory, { withFileTypes: true })) {
    if (!entry.isDirectory() || !entry.name.startsWith('whisper-rs-sys')) {
      continue;
    }

    const packageDirectory = join(buildDirectory, entry.name);
    const directCache = join(packageDirectory, 'out', 'build', 'CMakeCache.txt');
    if (existsSync(directCache)) {
      caches.add(directCache);
    }

    // Cargo 1.100+ places build-script output in
    // build/<crate-name>/<fingerprint>/out instead of the older
    // build/<crate-name>-<fingerprint>/out layout.
    for (const fingerprint of readdirSync(packageDirectory, { withFileTypes: true })) {
      if (!fingerprint.isDirectory()) {
        continue;
      }
      const nestedCache = join(
        packageDirectory,
        fingerprint.name,
        'out',
        'build',
        'CMakeCache.txt'
      );
      if (existsSync(nestedCache)) {
        caches.add(nestedCache);
      }
    }
  }

  return [...caches];
}

// Run before bundling so unverified native code cannot become a release asset.
try {
  if (process.argv.length !== 3) {
    throw new Error('Usage: node verify-portable-ggml.cjs <target/profile directory>');
  }

  const buildDirectory = resolve(process.argv[2], 'build');
  const caches = findWhisperCaches(buildDirectory);
  if (caches.length === 0) {
    throw new Error(
      'No Whisper CMake cache found in ' + buildDirectory +
      '. Checked both Cargo flat and nested build-script output layouts.'
    );
  }

  for (const cache of caches) {
    const contents = readFileSync(cache, 'utf8');
    console.log('Checking ' + cache);

    for (const line of contents.split(/\r?\n/)) {
      if (/^(GGML_(NATIVE|AVX[^:]*|FMA|F16C)|CMAKE_C(?:XX)?_FLAGS[^:]*):/.test(line)) {
        console.log(line);
      }
    }

    for (const flag of PORTABILITY_FLAGS) {
      if (!new RegExp('^' + flag + ':BOOL=OFF\\r?$', 'm').test(contents)) {
        throw new Error(flag + ' must be explicitly OFF in ' + cache);
      }
    }
  }

  console.log('Verified Windows Whisper CPU portability before bundling.');
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
