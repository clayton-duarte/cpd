import { useState } from "react";
import type {
  Dialog,
  DialogResponse,
  Question,
  QuestionAnswer,
} from "../../protocol/domain.ts";
import { Markdown } from "../chat/Markdown.tsx";
import { Button } from "../components/button.tsx";
import { ScrollArea, ScrollBar } from "../components/scroll-area.tsx";
import {
  Questionnaire,
  QuestionnaireActions,
  QuestionnaireChoice,
  QuestionnaireChoices,
  QuestionnaireDescription,
  QuestionnaireError,
  QuestionnaireInput,
  QuestionnaireItem,
  QuestionnaireNext,
  QuestionnairePrevious,
  QuestionnaireProgress,
  QuestionnaireSkip,
  QuestionnaireSubmit,
  QuestionnaireTitle,
} from "../components/questionnaire.tsx";

type AskDialog = Extract<Dialog, { method: "ask_user_question" }>;
const READ_ONLY_NOTE =
  "Answer in the session's own terminal; this view is read-only.";

function extractAnswer(
  q: Question,
  index: number,
  values: string[],
): QuestionAnswer | null {
  const optionLabels = new Set(q.options.map((o) => o.label));
  const selected = values.filter((v) => optionLabels.has(v));
  const custom =
    values
      .find((v) => !optionLabels.has(v) && v.trim() !== "")
      ?.trim() ?? "";

  if (q.multiSelect) {
    if (selected.length === 0 && custom === "") return null;
    return {
      questionIndex: index,
      question: q.question,
      kind: "multi",
      selected,
      answer: custom,
    };
  }
  if (custom !== "") {
    return {
      questionIndex: index,
      question: q.question,
      kind: "custom",
      answer: custom,
    };
  }
  const label = selected[0];
  if (label === undefined) return null;
  const preview = q.options.find((o) => o.label === label)?.preview;
  const base = {
    questionIndex: index,
    question: q.question,
    kind: "option" as const,
    answer: label,
  };
  return preview === undefined ? base : { ...base, preview };
}

export function AskCard(props: {
  dialog: AskDialog;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  asker?: string;
  recommended?: readonly {
    readonly questionIndex: number;
    readonly label: string;
  }[];
}): React.JSX.Element {
  const { dialog, readOnly, onRespond, recommended } = props;
  const [focusedByQ, setFocusedByQ] = useState<Record<number, string>>({});

  if (readOnly) {
    return (
      <section
        aria-label="Questions"
        data-dialog="ask_user_question"
        data-readonly=""
        className="mx-4 mb-3 shrink-0 rounded-2xl border bg-card p-5"
      >
        <div className="flex flex-col gap-3">
          <ol className="m-0 flex list-none flex-col gap-3 p-0">
            {dialog.questions.map((q, i) => (
              <li key={i}>
                <p className="font-medium text-fg-strong">
                  {q.header}: {q.question}
                </p>
                <ul className="text-sm text-muted-foreground">
                  {q.options.map((o) => (
                    <li key={o.label}>
                      {o.label} - {o.description}
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ol>
          <p
            data-part="note"
            className="font-mono text-xs text-muted-foreground"
          >
            {READ_ONLY_NOTE}
          </p>
        </div>
      </section>
    );
  }

  const cancel = (): void =>
    onRespond({ method: "ask_user_question", outcome: "cancelled" });

  const questionnaireItems = dialog.questions.map((q, i) => ({
    name: `q${i}`,
    title: `${q.header}: ${q.question}`,
    description: q.multiSelect
      ? `${q.header} · Select all that apply, or type your own.`
      : `${q.header} · Choose an option, or type your own.`,
    multiple: q.multiSelect === true,
    required: false,
    choices: q.options.map((o) => ({
      value: o.label,
      label: o.label,
      description: o.description,
      preview: o.preview,
    })),
    input: {
      label: "Type something.",
      placeholder: "Type something.",
    },
  }));

  return (
    <ScrollArea className="mx-4 mb-3 shrink-0 rounded-2xl border bg-card [&>[data-slot=scroll-area-viewport]]:max-h-[min(70vh,560px)]">
    <div className="p-5">
      <Questionnaire
        aria-label="Questions"
        data-dialog="ask_user_question"
        defaultItem={questionnaireItems[0]?.name}
        items={questionnaireItems}
        shortcuts="letters"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            cancel();
          }
        }}
        onSubmit={(event) => {
          event.preventDefault();
          const formData = new FormData(event.currentTarget);
          const out: QuestionAnswer[] = [];
          const unanswered: number[] = [];
          dialog.questions.forEach((q, i) => {
            const raw = formData
              .getAll(`q${i}`)
              .map((v) => String(v))
              .filter((v) => v !== "");
            const ans = extractAnswer(q, i, raw);
            if (ans !== null) out.push(ans);
            else unanswered.push(i);
          });
          onRespond({
            method: "ask_user_question",
            outcome: "answered",
            result: {
              answers: out,
              cancelled: false,
              ...(unanswered.length > 0 ? { unanswered } : {}),
            },
          });
        }}
      >
        <QuestionnaireProgress />
        {questionnaireItems.map((question, i) => {
          const hasPreview = question.choices.some(
            (c) => c.preview !== undefined,
          );
          const focusedLabel = focusedByQ[i];
          const preview = question.choices.find(
            (c) => c.value === focusedLabel,
          )?.preview;

          return (
            <QuestionnaireItem
              key={question.name}
              multiple={question.multiple}
              name={question.name}
              required={question.required}
              data-question={i}
            >
              <QuestionnaireTitle>{question.title}</QuestionnaireTitle>
              <QuestionnaireDescription>
                {question.description}
              </QuestionnaireDescription>
              <div
                data-layout={hasPreview ? "side-by-side" : "single"}
                className={hasPreview ? undefined : "flex w-full flex-col"}
              >
                <QuestionnaireChoices data-part="options">
                  {question.choices.map((choice) => {
                    const rec =
                      recommended?.some(
                        (r) =>
                          r.questionIndex === i && r.label === choice.label,
                      ) === true;
                    return (
                      <QuestionnaireChoice
                        key={choice.value}
                        value={choice.value}
                        data-part="option"
                        data-option={choice.label}
                        onClick={() =>
                          setFocusedByQ((prev) => ({
                            ...prev,
                            [i]: choice.label,
                          }))
                        }
                        onMouseEnter={() =>
                          setFocusedByQ((prev) => ({
                            ...prev,
                            [i]: choice.label,
                          }))
                        }
                        onFocus={() =>
                          setFocusedByQ((prev) => ({
                            ...prev,
                            [i]: choice.label,
                          }))
                        }
                      >
                        <span className="font-medium">
                          {choice.label}
                          {rec ? " (Recommended)" : ""}
                        </span>
                        <span className="text-muted-foreground">
                          {choice.description}
                        </span>
                      </QuestionnaireChoice>
                    );
                  })}
                  <QuestionnaireInput
                    aria-label={question.input.label}
                    placeholder={question.input.placeholder}
                  />
                </QuestionnaireChoices>
                {hasPreview ? (
                  <div
                    data-part="preview-pane"
                    className="flex min-w-0 flex-col rounded-2xl border bg-background p-4"
                  >
                    <div
                      data-part="preview-header"
                      className="mb-2 flex items-center justify-between border-b pb-2 text-xs text-muted-foreground"
                    >
                      <span className="font-mono uppercase">Preview</span>
                      <span
                        data-part="preview-focus"
                        className="font-mono text-foreground"
                      >
                        {focusedLabel ?? ""}
                      </span>
                    </div>
                    <ScrollArea
                      data-part="preview"
                      className="min-h-0 flex-1 text-xs"
                    >
                      {preview === undefined ? null : (
                        <Markdown text={preview} />
                      )}
                      <ScrollBar orientation="horizontal" />
                    </ScrollArea>
                  </div>
                ) : null}
              </div>
              <QuestionnaireError />
            </QuestionnaireItem>
          );
        })}
        <QuestionnaireActions>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            data-action="cancel"
            className="mr-auto"
            onClick={cancel}
          >
            Cancel
          </Button>
          <QuestionnairePrevious />
          <QuestionnaireSkip />
          <QuestionnaireNext />
          <QuestionnaireSubmit />
        </QuestionnaireActions>
      </Questionnaire>
    </div>
    </ScrollArea>
  );
}
