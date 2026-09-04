import type { LspTextEdit } from "../ipc";

/**
 * Apply LSP TextEdits to a JS string. Positions are UTF-16 code units
 * (LSP and JS strings agree), so offsets map 1:1.
 *
 * Edits are applied bottom-up: servers may return overlapping-inclusive
 * ranges; sorting by descending start makes each application safe for the
 * offsets of the edits still pending.
 */

interface Pos {
  line: number; // 0-based
  character: number; // 0-based UTF-16
}

function offsetOf(text: string, pos: Pos): number {
  const lines = text.split("\n");
  const line = Math.min(Math.max(0, pos.line), lines.length - 1);
  let offset = 0;
  for (let i = 0; i < line; i++) offset += lines[i].length + 1;
  return offset + Math.min(Math.max(0, pos.character), lines[line].length);
}

export function applyTextEdits(text: string, edits: LspTextEdit[]): string {
  if (!edits.length) return text;
  const sorted = [...edits].sort((a, b) => {
    const s1 = a.range.start;
    const s2 = b.range.start;
    return s1.line !== s2.line
      ? s2.line - s1.line
      : s2.character - s1.character;
  });
  let out = text;
  for (const e of sorted) {
    const start = offsetOf(out, e.range.start);
    const end = offsetOf(out, e.range.end);
    if (end < start) continue; // malformed edit — skip
    out = out.slice(0, start) + e.newText + out.slice(end);
  }
  return out;
}
