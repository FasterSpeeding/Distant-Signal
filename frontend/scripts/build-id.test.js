import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { computeBuildId, inputFiles, isInput } from './build-id.mjs';

// A plain `.test.js` for the same reason as next.config.test.js: the module
// under test is `.mjs`, outside tsconfig.json's program; tsconfig.scripts.json
// type-checks both.

/** @type {string} */
let dir;

/**
 * Write `content` to `rel` under the temp frontend dir.
 * @param {string} rel
 * @param {string} content
 */
function write(rel, content) {
  mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
  writeFileSync(path.join(dir, rel), content);
}

function hasGit() {
  try {
    execFileSync('git', ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

beforeEach(() => {
  dir = mkdtempSync(path.join(tmpdir(), 'build-id-'));
  write('package.json', '{"name":"frontend"}\n');
  write('package-lock.json', '{"lockfileVersion":3}\n');
  write('app/page.tsx', 'export default function Page() { return null; }\n');
  write('public/sw.js', "const CACHE_NAME = 'ds-__BUILD_ID__';\n");
  write('.env.example', 'API_BASE_URL=\n');
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe('computeBuildId', () => {
  it('is 24 hex characters and the same for the same inputs', () => {
    const id = computeBuildId(dir, { useGit: false });
    expect(id).toMatch(/^[0-9a-f]{24}$/);
    expect(computeBuildId(dir, { useGit: false })).toBe(id);
  });

  it('changes when a source file, the lockfile or a file name changes', () => {
    const before = computeBuildId(dir, { useGit: false });
    write('app/page.tsx', 'export default function Page() { return 1; }\n');
    const afterSource = computeBuildId(dir, { useGit: false });
    expect(afterSource).not.toBe(before);
    write('package-lock.json', '{"lockfileVersion":3,"packages":{}}\n');
    const afterLock = computeBuildId(dir, { useGit: false });
    expect(afterLock).not.toBe(afterSource);
    write('app/other.tsx', '');
    expect(computeBuildId(dir, { useGit: false })).not.toBe(afterLock);
  });

  it('ignores build output, dependencies and local env files', () => {
    const before = computeBuildId(dir, { useGit: false });
    write('node_modules/next/index.js', 'x');
    write('.next/BUILD_ID', 'old');
    write('.next/cache/x', 'x');
    write('next-env.d.ts', '/// <reference types="next" />\n');
    write('tsconfig.tsbuildinfo', '{}');
    write('test-results/a.txt', 'x');
    write('e2e-report/index.html', 'x');
    write('.env', 'SECRET=1\n');
    write('.env.local', 'SECRET=1\n');
    expect(computeBuildId(dir, { useGit: false })).toBe(before);
  });

  it('gives the same id from git and from the walk on a clean tree', () => {
    if (!hasGit()) return;
    const git = (/** @type {string[]} */ ...args) =>
      execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args], {
        cwd: dir,
        stdio: 'ignore',
      });
    write('.gitignore', 'node_modules/\n.next/\n.env\nnext-env.d.ts\n');
    git('init', '-q');
    git('add', '-A');
    git('commit', '-q', '-m', 'init');
    // Ignored or excluded either way, as in a real checkout after a build.
    write('node_modules/x.js', 'x');
    write('.next/BUILD_ID', 'x');
    expect(inputFiles(dir)).toEqual(inputFiles(dir, { useGit: false }));
    expect(computeBuildId(dir)).toBe(computeBuildId(dir, { useGit: false }));
    // An uncommitted edit counts under git too.
    const clean = computeBuildId(dir);
    write('app/page.tsx', 'changed\n');
    expect(computeBuildId(dir)).not.toBe(clean);
  });
});

describe('isInput', () => {
  it('keeps sources, configs and the env template', () => {
    for (const rel of ['app/page.tsx', 'next.config.mjs', 'public/sw.js', 'Dockerfile', '.env.example']) {
      expect(isInput(rel)).toBe(true);
    }
  });

  it('drops excluded directories at any depth', () => {
    for (const rel of ['node_modules/a.js', 'app/node_modules/a.js', '.next/x', 'e2e-report/x', 'test-results/x']) {
      expect(isInput(rel)).toBe(false);
    }
  });
});
