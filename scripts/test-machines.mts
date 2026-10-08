// Unit tests for src/machines.ts. Run: npm test
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { naturalCompare, searchModels, shortModelName, sortCatalog, squash } from "../src/machines.ts";

// Real catalog data, in the same shape the backend sends.
type Raw = { m: string; p: { n: string; w: number }[] };
const raw: Raw[] = JSON.parse(readFileSync(new URL("../crates/signcut-core/data/machines.json", import.meta.url), "utf8"));
const catalog = sortCatalog(
  // Shuffle manufacturers to prove sorting does not rely on input order.
  [...raw].reverse().map((d) => ({ manufacturer: d.m, models: d.p.map((p) => ({ name: p.n, maxWidthMm: p.w })) })),
);

let n = 0;
const test = (name: string, fn: () => void) => {
  fn();
  n++;
  console.log("ok -", name);
};

test("natural order", () => {
  const v = ["KH-1350", "KH-375", "KH-720", "kh-870"].sort(naturalCompare);
  assert.deepEqual(v, ["KH-375", "KH-720", "kh-870", "KH-1350"]);
});

test("manufacturers sorted case-insensitively", () => {
  const names = catalog.map((c) => c.manufacturer);
  const expected = [...names].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase(), "en", { numeric: true }));
  assert.deepEqual(names, expected);
  assert.equal(names[0], "Accugraphics");
});

test("VEVOR models sorted naturally", () => {
  const v = catalog.find((c) => c.manufacturer === "VEVOR")!.models.map((m) => m.name);
  const i375 = v.indexOf("VEVOR KH-375");
  const i720 = v.indexOf("VEVOR KH-720");
  const i1350 = v.indexOf("VEVOR KH-1350");
  assert.ok(i375 < i720 && i720 < i1350, v.slice(0, 12).join(", "));
  // Variants sit next to their base model.
  assert.equal(v[v.indexOf("VEVOR KH-720") + 1], "VEVOR KH-720A");
});

test("duplicate model names are removed", () => {
  const c = sortCatalog([{ manufacturer: "X", models: [{ name: "A", maxWidthMm: 1 }, { name: "A", maxWidthMm: 2 }] }]);
  assert.equal(c[0].models.length, 1);
  for (const m of catalog) assert.equal(new Set(m.models.map((x) => x.name)).size, m.models.length, m.manufacturer);
});

test("squash ignores case, spaces and punctuation", () => {
  assert.equal(squash("VEVOR KH-720 A"), "vevorkh720a");
});

test("search across manufacturers with flexible spelling", () => {
  for (const q of ["kh720", "KH-720", "vevor 720", "720 vevor"]) {
    const hits = searchModels(catalog, q);
    assert.ok(hits.some((h) => h.model.name === "VEVOR KH-720"), q);
    assert.ok(hits.every((h) => squash(h.manufacturer + h.model.name).includes("720")), q);
  }
});

test("exact model matches rank first", () => {
  // The same OEM machine is sold as "E-CUT KH-720" and "VEVOR KH-720".
  const hits = searchModels(catalog, "kh-720");
  const top = hits.slice(0, 2).map((h) => h.model.name).sort();
  assert.deepEqual(top, ["E-CUT KH-720", "VEVOR KH-720"]);
  // Partial matches (KH-720A, KH-720D, …) come after the exact ones.
  assert.ok(hits.slice(2).every((h) => h.score < hits[1].score));
});

test("browsed manufacturer wins ties", () => {
  assert.equal(searchModels(catalog, "kh-720", 300, "VEVOR")[0].model.name, "VEVOR KH-720");
  assert.equal(searchModels(catalog, "kh-720", 300, "E-CUT")[0].model.name, "E-CUT KH-720");
});

test("manufacturer-only query lists that maker's models", () => {
  const hits = searchModels(catalog, "vevor", 1000);
  const vevor = catalog.find((c) => c.manufacturer === "VEVOR")!.models.length;
  assert.ok(hits.length >= vevor && vevor > 90, `${hits.length} hits, ${vevor} VEVOR models`);
  assert.ok(hits.every((h) => h.manufacturer === "VEVOR" || squash(h.model.name).includes("vevor")));
});

test("no match / empty query", () => {
  assert.equal(searchModels(catalog, "zzzznotacutter").length, 0);
  assert.equal(searchModels(catalog, "   ").length, 0);
});

test("short model name", () => {
  assert.equal(shortModelName("VEVOR", "VEVOR KH-720"), "KH-720");
  assert.equal(shortModelName("Graphtec", "CE6000-60"), "CE6000-60");
});

console.log(`${n} tests passed`);
