// Verify that every translatable string has a German translation.
//   node scripts/check-i18n.mjs          -> exit 1 if something is missing
//   node scripts/check-i18n.mjs --list   -> print all keys
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const files = [];
(function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) {
      if (f !== "locales" && f !== "mock") walk(p);
    } else if (/\.(ts|tsx)$/.test(f)) files.push(p);
  }
})(join(root, "src"));

const keys = new Set();
const unescape = (s) => JSON.parse(`"${s}"`);
for (const f of files) {
  const src = readFileSync(f, "utf8");
  for (const m of src.matchAll(/\bt\(\s*"((?:[^"\\]|\\.)*)"/g)) keys.add(unescape(m[1]));
}
// Backend message templates in src/i18n.ts.
const i18n = readFileSync(join(root, "src/i18n.ts"), "utf8");
const block = i18n.slice(i18n.indexOf("const backendRules"), i18n.indexOf("/** Translate a message from the backend"));
for (const m of block.matchAll(/\/[a-z]*,\s*"((?:[^"\\]|\\.)*)",?\s*\]/g)) keys.add(unescape(m[1]));

if (process.argv.includes("--list")) {
  for (const k of [...keys].sort()) console.log(JSON.stringify(k));
  process.exit(0);
}

const deSrc = readFileSync(join(root, "src/locales/de.ts"), "utf8");
const de = new Set();
for (const m of deSrc.matchAll(/^\s*("(?:[^"\\]|\\.)*")\s*:/gm)) de.add(JSON.parse(m[1]));
const missing = [...keys].filter((k) => !de.has(k));
// Placeholders must survive translation.
const bad = [];
for (const m of deSrc.matchAll(/^\s*("(?:[^"\\]|\\.)*")\s*:\s*("(?:[^"\\]|\\.)*")/gm)) {
  const [k, v] = [JSON.parse(m[1]), JSON.parse(m[2])];
  const ph = (s) => [...s.matchAll(/\{\w+\}/g)].map((x) => x[0]).sort().join(",");
  if (ph(k) !== ph(v)) bad.push(`${k}  ->  ${v}`);
}
if (missing.length || bad.length) {
  if (missing.length) console.error(`Missing German translations (${missing.length}):\n` + missing.map((k) => "  " + JSON.stringify(k)).join("\n"));
  if (bad.length) console.error(`Placeholder mismatch:\n  ` + bad.join("\n  "));
  process.exit(1);
}
console.log(`i18n OK: ${keys.size} strings translated.`);
