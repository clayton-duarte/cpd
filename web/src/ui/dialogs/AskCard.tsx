import { useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Dialog, DialogResponse, Question, QuestionAnswer } from "../../protocol/domain.ts";

type AskDialog = Extract<Dialog, { method: "ask_user_question" }>;
interface Pick {
  readonly selected: readonly string[];
  readonly custom: string;
  readonly focused: string | null;
}
const EMPTY: Pick = { selected: [], custom: "", focused: null };
const READ_ONLY_NOTE = "Answer in the session's own terminal; this view is read-only.";

function answerFor(q: Question, index: number, pick: Pick): QuestionAnswer | null {
  const custom = pick.custom.trim();
  if (q.multiSelect) {
    if (pick.selected.length === 0 && custom === "") return null;
    const selected = q.options.map((o) => o.label).filter((l) => pick.selected.includes(l));
    return { questionIndex: index, question: q.question, kind: "multi", selected, answer: custom };
  }
  if (custom !== "") return { questionIndex: index, question: q.question, kind: "custom", answer: custom };
  const label = pick.selected[0];
  if (label === undefined) return null;
  const preview = q.options.find((o) => o.label === label)?.preview;
  const base = { questionIndex: index, question: q.question, kind: "option" as const, answer: label };
  return preview === undefined ? base : { ...base, preview };
}

export function AskCard(props: {
  dialog: AskDialog;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  asker?: string;
  recommended?: readonly { readonly questionIndex: number; readonly label: string }[];
}): React.JSX.Element {
  const { dialog, readOnly, onRespond } = props;
  const [picks, setPicks] = useState<readonly Pick[]>(() => dialog.questions.map(() => EMPTY));

  if (readOnly) {
    return (
      <section aria-label="Questions" data-dialog="ask_user_question" data-readonly="">
        <ol>
          {dialog.questions.map((q, i) => (
            <li key={i}>
              <p>
                {q.header}: {q.question}
              </p>
              <ul>
                {q.options.map((o) => (
                  <li key={o.label}>
                    {o.label} - {o.description}
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ol>
        <p>{READ_ONLY_NOTE}</p>
      </section>
    );
  }

  const update = (i: number, fn: (p: Pick) => Pick) =>
    setPicks((all) => all.map((p, j) => (j === i ? fn(p) : p)));
  const cancel = () => onRespond({ method: "ask_user_question", outcome: "cancelled" });

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const answers: QuestionAnswer[] = [];
    const unanswered: number[] = [];
    dialog.questions.forEach((q, i) => {
      const a = answerFor(q, i, picks[i] ?? EMPTY);
      if (a) answers.push(a);
      else unanswered.push(i);
    });
    onRespond({
      method: "ask_user_question",
      outcome: "answered",
      result: { answers, cancelled: false, ...(unanswered.length > 0 ? { unanswered } : {}) },
    });
  };

  return (
    <form
      aria-label="Questions"
      data-dialog="ask_user_question"
      onSubmit={submit}
      onKeyDown={(e) => {
        if (e.key === "Escape") cancel();
      }}
    >
      {dialog.questions.map((q, i) => {
        const pick = picks[i] ?? EMPTY;
        const multi = q.multiSelect === true;
        const preview = q.options.find((o) => o.label === pick.focused)?.preview;
        const choose = (label: string) =>
          update(i, (p) =>
            multi
              ? {
                  ...p,
                  focused: label,
                  selected: p.selected.includes(label) ? p.selected.filter((l) => l !== label) : [...p.selected, label],
                }
              : { selected: [label], custom: "", focused: label },
          );
        return (
          <fieldset key={i} data-question={i}>
            <legend>
              {q.header}: {q.question}
            </legend>
            <div data-layout="side-by-side">
              <div data-part="options">
                {q.options.map((o) => (
                  <label key={o.label}>
                    <input
                      type={multi ? "checkbox" : "radio"}
                      name={`${dialog.id}-q${i}`}
                      value={o.label}
                      checked={pick.selected.includes(o.label)}
                      onChange={() => choose(o.label)}
                      onFocus={() => update(i, (p) => ({ ...p, focused: o.label }))}
                    />
                    {o.label} <span>{o.description}</span>
                  </label>
                ))}
                <label>
                  <input
                    type="text"
                    aria-label="Type something."
                    placeholder="Type something."
                    value={pick.custom}
                    onChange={(e) =>
                      update(i, (p) => ({ ...p, custom: e.target.value, selected: multi ? p.selected : [] }))
                    }
                  />
                </label>
              </div>
              <div data-part="preview">
                {preview === undefined ? null : <Markdown remarkPlugins={[remarkGfm]}>{preview}</Markdown>}
              </div>
            </div>
          </fieldset>
        );
      })}
      <button type="submit">Submit</button>
      <button type="button" data-action="cancel" onClick={cancel}>
        Cancel
      </button>
    </form>
  );
}
