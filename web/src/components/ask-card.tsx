import type { ReactElement } from "react";
import { CheckIcon, CircleQuestionMarkIcon } from "lucide-react";

import { Badge } from "./ui/badge";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from "./ui/item";
import type { TranscriptAskAnswer, TranscriptAskQuestion, TranscriptToolPart } from "../transcript";

/**
 * An `ask_user_question` call rendered as the question and its answer, not as a JSON tool blob.
 *
 * The transcript is history, so there is nothing to approve: the call's arguments carry the
 * questions and their options, and the result's `details.answers` carries what the person chose —
 * `kind: "option"` a picked label, `"custom"` a typed answer, `"multi"` the picked labels with an
 * optional typed one — so each question shows its choices with the taken one marked. A question
 * that was never answered, and an ask the guard refused, say so instead of inventing an answer.
 *
 * The shape follows the registry's Confirmation element (an `Item` with `ItemMedia`,
 * `ItemContent` and `ItemActions`); only the actions differ, since a recorded answer has no
 * Approve or Deny button left to press.
 */
export function AskCard({ part }: { part: TranscriptToolPart }): ReactElement | null {
  const questions = part.questions ?? [];
  if (questions.length === 0) return null;
  const byIndex = new Map((part.answers ?? []).map((answer) => [answer.questionIndex, answer]));

  return (
    <ItemGroup className="gap-2">
      {questions.map((question, index) => (
        <AskItem key={index} question={question} answer={byIndex.get(index)} part={part} />
      ))}
    </ItemGroup>
  );
}

function AskItem({
  question,
  answer,
  part,
}: {
  question: TranscriptAskQuestion;
  answer?: TranscriptAskAnswer;
  part: TranscriptToolPart;
}): ReactElement {
  const refused = part.state === "error";
  const picked = new Set(
    answer === undefined
      ? []
      : answer.kind === "multi"
        ? (answer.selected ?? [])
        : answer.answer === null
          ? []
          : [answer.answer]
  );
  // The typed half of an answer: the whole answer for `custom`, an addition for `multi`.
  const typed = answer !== undefined && answer.kind !== "option" ? answer.answer : null;

  return (
    <Item variant="outline" size="sm" className="items-start">
      <ItemMedia variant="icon">
        <CircleQuestionMarkIcon />
      </ItemMedia>
      <ItemContent className="gap-2">
        <ItemTitle className="flex-wrap items-start gap-2 leading-snug">
          {question.header === "" ? null : (
            <Badge variant="secondary" className="align-middle">
              {question.header}
            </Badge>
          )}
          <span className="text-sm">{question.question}</span>
        </ItemTitle>
        <ul className="flex flex-col gap-1.5">
          {question.options.map((option) => {
            const chosen = picked.has(option.label);
            return (
              <li key={option.label} className="flex items-start gap-2">
                <CheckIcon
                  aria-hidden={!chosen}
                  className={
                    chosen
                      ? "mt-0.5 size-3.5 shrink-0 text-foreground"
                      : "mt-0.5 size-3.5 shrink-0 text-transparent"
                  }
                />
                <span className="min-w-0">
                  <span
                    className={
                      chosen ? "text-sm font-medium text-foreground" : "text-sm text-muted-foreground"
                    }
                  >
                    {option.label}
                  </span>
                  {option.description === undefined ? null : (
                    <span className="block text-xs text-muted-foreground">{option.description}</span>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
        {/* A typed answer is the person's own words, so it reads as a quote, not as an option. */}
        {typed === null ? null : (
          <p className="border-s-2 border-border ps-2 text-sm whitespace-pre-wrap">{typed}</p>
        )}
        {refused ? (
          <p className="text-destructive text-xs">{part.output ?? "The ask was refused."}</p>
        ) : null}
      </ItemContent>
      <ItemActions>
        <Badge variant={refused ? "destructive" : answer === undefined ? "outline" : "secondary"}>
          {answerLabel(question, answer, part)}
        </Badge>
      </ItemActions>
    </Item>
  );
}

/** What the answer badge says: the pick, the count, the kind of typed answer, or that there is none. */
function answerLabel(
  question: TranscriptAskQuestion,
  answer: TranscriptAskAnswer | undefined,
  part: TranscriptToolPart
): string {
  if (part.state === "error") return "Refused";
  if (part.state === "running") return "Asking";
  if (answer === undefined || (answer.answer === null && (answer.selected ?? []).length === 0)) {
    return "No answer";
  }
  if (answer.kind === "multi") {
    return `${answer.selected?.length ?? 0} of ${question.options.length} picked`;
  }
  if (answer.kind === "custom") return "Typed answer";
  return answer.answer ?? "No answer";
}
