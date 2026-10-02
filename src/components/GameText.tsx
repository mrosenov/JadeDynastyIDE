import { styledLines, textLines } from "../elements/text";

interface Props {
  text: string;
  /** Show the colour codes instead of applying them. */
  raw?: boolean;
  className?: string;
}

/** A game text as the client shows it: line breaks and ^RRGGBB colours applied. */
export function GameText({ text, raw = false, className }: Props) {
  return (
    <div className={"game-text" + (className ? ` ${className}` : "")}>
      {raw
        ? textLines(text).map((line, i) => (
            <div key={i} className="text-line mono">
              {line || " "}
            </div>
          ))
        : styledLines(text).map((runs, i) => (
            <div key={i} className="text-line">
              {runs.length
                ? runs.map((r, j) => (
                    <span key={j} style={r.colour ? { color: r.colour } : undefined}>
                      {r.text}
                    </span>
                  ))
                : " "}
            </div>
          ))}
    </div>
  );
}
