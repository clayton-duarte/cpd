/** The tokenizer shared by the guards of commands.md §15.11, §15.12 and §15.16. */

/** Words a simple command may start with that are not the command (`FOO=1 sudo timeout 60 x`, `do cargo test`). */
export const PREFIXES = new Set(["do", "then", "else", "elif", "if", "!", "{", "}", "time", "nohup", "exec", "command", "sudo", "env", "while", "until"]);

/** The simple commands of a shell line, as words with their quotes taken off. Separators: `;`, `&`, `|`, newline, parentheses, backticks (not inside quotes). */
export function simpleCommands(line: string): string[][] {
  const commands: string[][] = [];
  let words: string[] = [];
  let word: string | undefined;
  let quote: string | undefined;
  const endWord = () => {
    if (word !== undefined) words.push(word);
    word = undefined;
  };
  const endCommand = () => {
    endWord();
    if (words.length > 0) commands.push(words);
    words = [];
  };
  const pending: { delim: string; strip: boolean }[] = [];
  for (let i = 0; i < line.length; i++) {
    const c = line[i]!;
    if (quote === undefined && c === "<" && line[i + 1] === "<" && line[i + 2] !== "<" && line[i - 1] !== "<") {
      const op = heredocOperator(line, i + 2);
      if (op) pending.push(op);
    }
    if (c === "\n" && quote === undefined && pending.length > 0) {
      endCommand();
      i = skipBodies(line, i + 1, pending.splice(0)) - 1;
      continue;
    }
    if (quote === "'") {
      if (c === "'") quote = undefined;
      else word += c;
    } else if (quote === '"') {
      if (c === '"') quote = undefined;
      else if (c === "\\" && i + 1 < line.length) word += line[++i];
      else word += c;
    } else if (c === "'" || c === '"') {
      quote = c;
      word ??= "";
    } else if (c === "\\" && i + 1 < line.length) {
      word = (word ?? "") + line[++i];
    } else if (c === "#" && word === undefined) {
      while (i + 1 < line.length && line[i + 1] !== "\n") i++;
    } else if (c === " " || c === "\t") {
      endWord();
    } else if (";&|\n()`".includes(c)) {
      endCommand();
    } else {
      word = (word ?? "") + c;
    }
  }
  endCommand();
  return commands;
}

/** The delimiter after a `<<` (at `from`, just past it), read as a word with its quotes taken off. */
function heredocOperator(line: string, from: number): { delim: string; strip: boolean } | undefined {
  let i = from;
  const strip = line[i] === "-";
  if (strip) i++;
  while (line[i] === " " || line[i] === "\t") i++;
  let delim = "";
  let quote: string | undefined;
  for (; i < line.length; i++) {
    const c = line[i]!;
    if (quote === "'") {
      if (c === "'") quote = undefined;
      else delim += c;
    } else if (quote === '"') {
      if (c === '"') quote = undefined;
      else if (c === "\\" && i + 1 < line.length) delim += line[++i];
      else delim += c;
    } else if (c === "'" || c === '"') quote = c;
    else if (c === "\\" && i + 1 < line.length) delim += line[++i];
    else if (" \t\n;&|<>()".includes(c)) break;
    else delim += c;
  }
  return delim === "" ? undefined : { delim, strip };
}

/** The index just past the bodies of `heredocs`, in order, from the start of the first body at `from`. */
function skipBodies(line: string, from: number, heredocs: { delim: string; strip: boolean }[]): number {
  let i = from;
  for (const { delim, strip } of heredocs) {
    while (i < line.length) {
      let end = line.indexOf("\n", i);
      if (end === -1) end = line.length;
      let text = line.slice(i, end);
      if (strip) text = text.replace(/^\t+/, "");
      i = Math.min(end + 1, line.length);
      if (text === delim) break;
    }
  }
  return i;
}
