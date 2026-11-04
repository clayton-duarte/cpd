import { useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
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

const HINTS = "↑↓ move · 1–4 pick · ⇥ next question · ⏎ submit · esc cancel";
const BADGE =
  "grid size-5 shrink-0 place-items-center rounded-[4px] border border-edge font-mono text-[11px] text-muted-foreground";
const SHELL = "mx-4 mb-3 overflow-hidden rounded-xl border border-status-decision bg-card";

function TitleBar(props: { asker: string | undefined; count: number; children?: React.ReactNode }) {
  return (
    <div
      data-part="title-bar"
      className="flex h-11 shrink-0 items-center gap-2.5 border-b border-edge bg-status-decision/10 pl-3.5 pr-2"
    >
      <span data-part="phi" aria-hidden="true" className="font-display text-[18px] font-bold text-status-decision-bright">
        φ
      </span>
      <span data-part="title" className="font-semibold text-fg-strong">
        {props.asker === undefined ? "Questions for you" : `${props.asker} asks you`}
      </span>
      <span data-part="count" className="font-mono text-[11.5px] text-muted-foreground">
        {props.count === 1 ? "1 question" : `${props.count} questions`}
      </span>
      <span aria-hidden="true" className="flex-1" />
      {props.children}
    </div>
  );
}

export function AskCard(props: {
  dialog: AskDialog;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  asker?: string;
  recommended?: readonly { readonly questionIndex: number; readonly label: string }[];
}): React.JSX.Element {
  const { dialog, readOnly, onRespond, asker, recommended } = props;
  const [picks, setPicks] = useState<readonly Pick[]>(() => dialog.questions.map(() => EMPTY));
  const [current, setCurrent] = useState(0);
  const formRef = useRef<HTMLFormElement>(null);
  const total = dialog.questions.length;

  if (readOnly) {
    return (
      <section aria-label="Questions" data-dialog="ask_user_question" data-readonly="" className={SHELL}>
        <TitleBar asker={asker} count={total} />
        <div className="flex flex-col gap-3 p-4">
          <ol className="m-0 flex list-none flex-col gap-3 p-0">
            {dialog.questions.map((q, i) => (
              <li key={i}>
                <p className="font-medium text-fg-strong">
                  {q.header}: {q.question}
                </p>
                <ul className="text-key text-muted-foreground">
                  {q.options.map((o) => (
                    <li key={o.label}>
                      {o.label} - {o.description}
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ol>
          <p data-part="note" className="font-mono text-meta text-muted-foreground">
            {READ_ONLY_NOTE}
          </p>
        </div>
      </section>
    );
  }

  const update = (i: number, fn: (p: Pick) => Pick) =>
    setPicks((all) => all.map((p, j) => (j === i ? fn(p) : p)));
  const cancel = () => onRespond({ method: "ask_user_question", outcome: "cancelled" });
  const answers = dialog.questions.map((q, i) => answerFor(q, i, picks[i] ?? EMPTY));
  const answered = answers.filter((a) => a !== null).length;

  const submit = () => {
    const out: QuestionAnswer[] = [];
    const unanswered: number[] = [];
    answers.forEach((a, i) => {
      if (a) out.push(a);
      else unanswered.push(i);
    });
    onRespond({
      method: "ask_user_question",
      outcome: "answered",
      result: { answers: out, cancelled: false, ...(unanswered.length > 0 ? { unanswered } : {}) },
    });
  };

  const choose = (i: number, label: string) =>
    update(i, (p) =>
      dialog.questions[i]?.multiSelect === true
        ? {
            ...p,
            focused: label,
            selected: p.selected.includes(label) ? p.selected.filter((l) => l !== label) : [...p.selected, label],
          }
        : { selected: [label], custom: "", focused: label },
    );

  const items = (i: number): HTMLInputElement[] =>
    Array.from(
      formRef.current?.querySelectorAll<HTMLInputElement>(`fieldset[data-question="${i}"] [data-part="options"] input`) ?? [],
    );
  const goTo = (i: number) => {
    setCurrent(i);
    // Focus after the panel is unhidden.
    queueMicrotask(() => items(i)[0]?.focus());
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLFormElement>) => {
    if (e.key === "Escape") {
      cancel();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.nativeEvent.isComposing) return;
    const target = e.target as HTMLElement;
    const inTab = target.getAttribute("role") === "tab";
    if (inTab && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      const next = Math.min(total - 1, Math.max(0, current + (e.key === "ArrowRight" ? 1 : -1)));
      setCurrent(next);
      queueMicrotask(() => formRef.current?.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus());
      return;
    }
    if (e.key === "Enter") {
      if (target.tagName === "BUTTON") return;
      e.preventDefault();
      submit();
      return;
    }
    const list = items(current);
    const at = list.indexOf(target as HTMLInputElement);
    const inList = at >= 0;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (!inList) return;
      e.preventDefault();
      list[Math.min(list.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))]?.focus();
      return;
    }
    if (e.key === "Tab" && inList) {
      const next = current + (e.shiftKey ? -1 : 1);
      if (next < 0 || next >= total) return;
      e.preventDefault();
      goTo(next);
      return;
    }
    if (/^[1-9]$/.test(e.key) && !(target instanceof HTMLTextAreaElement) && !(target.tagName === "INPUT" && (target as HTMLInputElement).type === "text")) {
      const q = dialog.questions[current];
      const n = Number(e.key);
      if (!q) return;
      if (n <= q.options.length) {
        const label = q.options[n - 1]?.label;
        if (label === undefined) return;
        choose(current, label);
        list[n - 1]?.focus();
      } else if (n === q.options.length + 1) {
        list[list.length - 1]?.focus();
        e.preventDefault();
      }
    }
  };

  return (
    <form
      ref={formRef}
      aria-label="Questions"
      data-dialog="ask_user_question"
      className={cn(SHELL, "flex max-h-[min(70vh,560px)] min-h-0 flex-col")}
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      onKeyDown={onKeyDown}
    >
      <TitleBar asker={asker} count={total}>
        {total > 1 ? (
          <div role="tablist" aria-label="Question tabs" data-part="tabs" className="flex gap-1">
            {dialog.questions.map((q, i) => {
              const done = answers[i] !== null;
              const selected = i === current;
              return (
                <button
                  key={i}
                  type="button"
                  role="tab"
                  id={`${dialog.id}-tab-${i}`}
                  data-part="tab"
                  data-tab={i}
                  aria-selected={selected}
                  aria-controls={`${dialog.id}-panel-${i}`}
                  tabIndex={selected ? 0 : -1}
                  onClick={() => setCurrent(i)}
                  className={cn(
                    "flex h-[30px] items-center gap-[7px] rounded-[7px] border px-2.5",
                    selected
                      ? "border-line bg-surface-pressed text-fg-strong"
                      : "border-transparent text-muted-foreground",
                  )}
                >
                  <span
                    data-part="tab-mark"
                    data-state={done ? "done" : "open"}
                    aria-hidden="true"
                    className={cn("font-mono text-[11px]", done ? "text-status-done" : "text-fg-dim")}
                  >
                    {done ? "✓" : "○"}
                  </span>
                  <span className="sr-only">{done ? "answered" : "not answered"}</span>
                  <span className="font-mono text-[12px]">{q.header}</span>
                </button>
              );
            })}
          </div>
        ) : null}
      </TitleBar>
      <div data-part="body" className="flex min-h-0 flex-1 overflow-y-auto">
        {dialog.questions.map((q, i) => {
          const pick = picks[i] ?? EMPTY;
          const multi = q.multiSelect === true;
          const hasPreview = q.options.some((o) => o.preview !== undefined);
          const focusIndex = q.options.findIndex((o) => o.label === pick.focused);
          const preview = q.options[focusIndex]?.preview;
          const tabbed = total > 1;
          return (
            <fieldset
              key={i}
              data-question={i}
              {...(tabbed
                ? { id: `${dialog.id}-panel-${i}`, role: "tabpanel", "aria-labelledby": `${dialog.id}-tab-${i}` }
                : {})}
              hidden={i !== current}
              className="m-0 flex min-h-0 min-w-0 flex-1 border-0 p-0"
            >
              <div
                data-layout={hasPreview ? "side-by-side" : "single"}
                className={hasPreview ? undefined : "flex w-full flex-col"}
              >
                <div data-part="options" className="flex min-w-0 flex-col gap-1.5 p-4">
                  <legend className="mb-1 p-0 text-[16px] leading-[1.4] font-medium text-fg-strong">
                    <span className="sr-only">{q.header}: </span>
                    {q.question}
                  </legend>
                  <span data-part="mode" className="mb-1 font-mono text-[11px] text-muted-foreground">
                    {multi ? "multi-select · space toggles" : "single-select"}
                  </span>
                  {q.options.map((o, k) => {
                    const picked = pick.selected.includes(o.label);
                    const focused = pick.focused === o.label;
                    const rec = recommended?.some((r) => r.questionIndex === i && r.label === o.label) === true;
                    return (
                      <label
                        key={o.label}
                        data-part="option"
                        data-option={o.label}
                        data-state={picked ? "picked" : "idle"}
                        data-focused={focused ? "true" : "false"}
                        className={cn(
                          "flex cursor-pointer gap-3 rounded-lg border px-3 py-2.5 hover:bg-row-hover",
                          focused ? "border-ramune-bright bg-status-focus/10" : "border-border",
                        )}
                      >
                        <input
                          type={multi ? "checkbox" : "radio"}
                          name={`${dialog.id}-q${i}`}
                          value={o.label}
                          className="peer sr-only"
                          checked={picked}
                          onChange={() => choose(i, o.label)}
                          onFocus={() => update(i, (p) => ({ ...p, focused: o.label }))}
                        />
                        <span data-part="badge" className={BADGE}>
                          {k + 1}
                        </span>
                        <span data-part="option-text" className="flex min-w-0 flex-1 flex-col gap-[3px]">
                          <span className="flex items-center gap-2">
                            <span className="font-semibold text-fg-strong">{o.label}</span>
                            {rec ? (
                              <span
                                data-part="recommended"
                                className="rounded-[4px] border border-status-decision/50 px-[5px] font-mono text-[10.5px] text-status-decision-bright"
                              >
                                recommended
                              </span>
                            ) : null}
                          </span>
                          <span className="text-key leading-[1.45] text-muted-foreground">{o.description}</span>
                        </span>
                        <span
                          data-part="mark"
                          aria-hidden="true"
                          className={cn(
                            "mt-0.5 box-border grid size-4 shrink-0 place-items-center border-[1.5px]",
                            picked ? "border-ramune-bright" : "border-line",
                            multi ? "rounded-[4px]" : "rounded-full",
                          )}
                        >
                          {picked ? (
                            <span className={cn("size-2 bg-ramune-bright", multi ? "rounded-[2px]" : "rounded-full")} />
                          ) : null}
                        </span>
                      </label>
                    );
                  })}
                  <div
                    data-part="custom"
                    className="flex items-center gap-3 rounded-lg border border-dashed border-line px-3 py-2.5"
                  >
                    <span data-part="badge" className={BADGE}>
                      {q.options.length + 1}
                    </span>
                    <input
                      type="text"
                      aria-label="Type something."
                      placeholder="Type something."
                      className="min-w-0 flex-1 bg-transparent text-[13px] leading-[1.45] text-fg-strong placeholder:text-fg-dim"
                      value={pick.custom}
                      onChange={(e) =>
                        update(i, (p) => ({ ...p, custom: e.target.value, selected: multi ? p.selected : [] }))
                      }
                    />
                  </div>
                </div>
                {hasPreview ? (
                  <div data-part="preview-pane" className="flex min-w-0 flex-col bg-background">
                    <div data-part="preview-header" className="flex h-[38px] items-center gap-2.5 border-b border-border px-4">
                      <span className="text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground">
                        Preview
                      </span>
                      <span data-part="preview-focus" className="font-mono text-[12px] text-fg-strong">
                        {focusIndex >= 0 ? `${focusIndex + 1} · ${pick.focused}` : ""}
                      </span>
                      <span className="flex-1" />
                      <span className="font-mono text-meta text-muted-foreground">follows focus · ⇥ to compare</span>
                    </div>
                    <div
                      data-part="preview"
                      className="typeset typeset-chat min-h-0 flex-1 overflow-auto px-5 py-[18px] text-[12.5px] leading-[1.55] [&_pre]:text-[12.5px] [&_pre]:leading-[1.55]"
                    >
                      {preview === undefined ? null : <Markdown remarkPlugins={[remarkGfm]}>{preview}</Markdown>}
                    </div>
                  </div>
                ) : null}
              </div>
            </fieldset>
          );
        })}
      </div>
      <div data-part="footer" className="flex h-[52px] shrink-0 items-center gap-3 border-t border-border px-3.5">
        <span data-part="hints" className="font-mono text-meta text-muted-foreground">
          {HINTS}
        </span>
        <span className="flex-1" />
        <Button type="button" variant="ghost" data-action="cancel" onClick={cancel} className="h-8 rounded-[7px] px-3.5 text-muted-foreground">
          Cancel
        </Button>
        <Button
          type="button"
          variant="outline"
          data-action="skip"
          className="h-8 rounded-[7px] border-edge px-3.5 text-fg-strong"
          onClick={() => {
            update(current, () => EMPTY);
            if (current < total - 1) setCurrent(current + 1);
          }}
        >
          Skip question
        </Button>
        <Button type="submit" className="h-8 rounded-[7px] bg-primary px-4 font-semibold text-primary-foreground">
          Submit {answered}/{total} answers <kbd className="font-mono text-[11px]">⏎</kbd>
        </Button>
      </div>
    </form>
  );
}
