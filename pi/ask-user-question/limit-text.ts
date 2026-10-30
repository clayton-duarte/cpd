import { LIMIT } from "../lib/askguard.ts";

/** The limit as the model reads it: in the tool description and the last prompt guideline. */
export const limitSentence = `The whole call holds at most ${LIMIT} characters, summed over every question, header, option label, description and preview; a longer call is refused before the User sees it. Put larger content in a file outside the repo and point to it in chat.`;

/** The note that ends the `question` and `preview` field descriptions. */
export const fieldNote = `Counts toward the call's ${LIMIT}-character total.`;
