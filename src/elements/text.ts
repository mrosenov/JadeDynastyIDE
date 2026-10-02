// Game texts: line breaks and the client's ^RRGGBB colour codes.
//
// elements.data stores line breaks as CR LF (sometimes a bare LF). The
// official editor's text exports write them as "!$", and a few of those
// made it into the data files, so all three count as a break.

import type { FieldNode } from "./types";

/** A text field: wchar[n] or char[n]. */
export const isTextNode = (node: FieldNode) => !node.children && /^w?char\[/.test(node.ty) && node.value !== undefined;

const BREAK = /\r\n|\r|\n|!\$/;
const COLOUR = /\^([0-9a-fA-F]{6})/;

/** The text as lines. */
export const textLines = (text: string) => text.split(new RegExp(BREAK, "g"));

export const hasBreaks = (text: string) => BREAK.test(text);
export const hasColours = (text: string) => COLOUR.test(text);

export interface Run {
  text: string;
  /** "#rrggbb", or undefined for the default colour. */
  colour?: string;
}

/** One line split at its colour codes; a code applies until the next one. */
export function colourRuns(line: string, colour?: string): Run[] {
  const runs: Run[] = [];
  const parts = line.split(new RegExp(COLOUR, "g"));
  // split() with a capture group alternates text and colour.
  for (let i = 0; i < parts.length; i++) {
    if (i % 2) colour = `#${parts[i].toLowerCase()}`;
    else if (parts[i]) runs.push({ text: parts[i], colour });
  }
  return runs;
}

/** Lines of runs; a colour lasts until the next code, across line breaks. */
export function styledLines(text: string): Run[][] {
  let colour: string | undefined;
  return textLines(text).map((line) => {
    const runs = colourRuns(line, colour);
    const last = [...line.matchAll(new RegExp(COLOUR, "g"))].pop();
    if (last) colour = `#${last[1].toLowerCase()}`;
    return runs;
  });
}
