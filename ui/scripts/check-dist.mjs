// Verifies the built sidebar against the packaging rules. Run after `vite build`; `pnpm build` does.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkDistFiles } from './check-dist-lib.mjs';

const dist = fileURLToPath(new URL('../dist/', import.meta.url));

function walk(dir) {
  const files = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) files.push(...walk(path));
    else files.push(path);
  }
  return files;
}

const contents = {};
for (const file of walk(dist)) {
  contents[relative(dist, file)] = readFileSync(file, 'utf8');
}

const problems = checkDistFiles(contents);
if (problems.length > 0) {
  console.error(`check-dist: ${problems.length} problem(s)`);
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}
console.log(`check-dist: ok (${Object.keys(contents).length} files, CSP verified, no external references)`);
