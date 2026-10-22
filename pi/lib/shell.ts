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
  for (let i = 0; i < line.length; i++) {
    const c = line[i]!;
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
