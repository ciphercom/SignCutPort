// Sorting and searching the cutter catalog (no React here, so it can be
// unit-tested with plain Node: `npm test`).

export interface ModelEntry {
  name: string;
  maxWidthMm: number;
}
export interface MakerEntry {
  manufacturer: string;
  models: ModelEntry[];
}

/** Natural, case-insensitive order: "KH-375" < "KH-720" < "KH-1350". */
const collator = new Intl.Collator("en", { numeric: true, sensitivity: "base" });
export const naturalCompare = (a: string, b: string) => collator.compare(a, b);

/** Manufacturers A–Z, each with its models de-duplicated and in natural order. */
export function sortCatalog<T extends MakerEntry>(makers: T[]): T[] {
  return [...makers]
    .map((m) => {
      const seen = new Set<string>();
      const models = m.models.filter((x) => !seen.has(x.name) && !!seen.add(x.name));
      return { ...m, models: models.sort((a, b) => naturalCompare(a.name, b.name)) };
    })
    .sort((a, b) => naturalCompare(a.manufacturer, b.manufacturer));
}

/** Lowercase and drop everything but letters and digits ("KH-720" -> "kh720"). */
export function squash(s: string): string {
  return s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
}

export interface SearchHit {
  manufacturer: string;
  model: ModelEntry;
  score: number;
}

/**
 * Search all models. Every whitespace-separated query token must occur in
 * "manufacturer + model" (ignoring case, spaces and punctuation), so
 * "vevor 720", "kh720" and "720a" all work. Results are ranked: exact model
 * match, then model starts with the query, then token matches in the model
 * name rather than only in the manufacturer, then the preferred manufacturer;
 * remaining ties keep catalog order.
 */
export function searchModels(makers: MakerEntry[], query: string, limit = 300, preferMaker?: string): SearchHit[] {
  const tokens = query.split(/\s+/).map(squash).filter(Boolean);
  if (!tokens.length) return [];
  const whole = squash(query);
  const hits: SearchHit[] = [];
  for (const m of makers) {
    const maker = squash(m.manufacturer);
    for (const model of m.models) {
      const name = squash(model.name);
      // Model names often repeat the maker ("VEVOR KH-720"); match against both.
      const hay = maker + name;
      if (!tokens.every((tk) => hay.includes(tk))) continue;
      const bare = name.startsWith(maker) ? name.slice(maker.length) : name;
      let score = 0;
      if (bare === whole || name === whole) score += 100;
      else if (bare.startsWith(whole) || name.startsWith(whole)) score += 50;
      score += tokens.filter((tk) => name.includes(tk)).length * 5;
      // On a tie, the manufacturer being browsed wins.
      if (preferMaker && m.manufacturer === preferMaker) score += 1;
      hits.push({ manufacturer: m.manufacturer, model, score });
    }
  }
  // Stable sort keeps the (sorted) catalog order for equal scores.
  hits.sort((a, b) => b.score - a.score);
  return hits.slice(0, limit);
}

/** "VEVOR KH-720" shown as "KH-720" under the manufacturer "VEVOR". */
export function shortModelName(manufacturer: string, model: string): string {
  const m = manufacturer.toLowerCase();
  return model.toLowerCase().startsWith(m + " ") ? model.slice(manufacturer.length + 1) : model;
}
