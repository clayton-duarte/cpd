// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { mount } from "../../testing/render.tsx";
import { linkifyMentions, MentionLink } from "./mentions.tsx";

describe("chat mentions", () => {
  it("renders agent keys in user assistant event and tool messages as router links", () => {
    const liveKeys = new Set(["cpd", "cpd:plan-x", "cpd:plan-x:web-tests"]);

    // 1. User message / plain text scenario
    const userText = "Ask cpd:plan-x for details and worker cpd:plan-x:web-tests for status.";
    const userResult = linkifyMentions(userText, liveKeys);
    const { container: userContainer, unmount: unmountUser } = mount(<div>{userResult}</div>);

    const userLinks = userContainer.querySelectorAll('a[data-part="agent-mention"]');
    expect(userLinks).toHaveLength(2);

    expect(userLinks[0]?.getAttribute("data-agent-key")).toBe("cpd:plan-x");
    expect(userLinks[0]?.getAttribute("href")).toBe("#/a/cpd/plan-x");
    expect(userLinks[0]?.textContent).toBe("cpd:plan-x");

    expect(userLinks[1]?.getAttribute("data-agent-key")).toBe("cpd:plan-x:web-tests");
    expect(userLinks[1]?.getAttribute("href")).toBe("#/a/cpd/plan-x/web-tests");
    expect(userLinks[1]?.textContent).toBe("cpd:plan-x:web-tests");
    unmountUser();

    // 2. Assistant message / manager mention scenario
    const assistantText = "Context manager is cpd. Follow up with cpd:plan-x.";
    const assistantResult = linkifyMentions(assistantText, liveKeys);
    const { container: assistantContainer, unmount: unmountAssistant } = mount(<div>{assistantResult}</div>);

    const assistantLinks = assistantContainer.querySelectorAll('a[data-part="agent-mention"]');
    expect(assistantLinks).toHaveLength(2);
    expect(assistantLinks[0]?.getAttribute("data-agent-key")).toBe("cpd");
    expect(assistantLinks[0]?.getAttribute("href")).toBe("#/a/cpd");
    expect(assistantLinks[0]?.textContent).toBe("cpd");
    unmountAssistant();

    // 3. Event / fold message scenario (e.g. FoldRow, CompactionRow, OrphanRow)
    const eventText = "Handed off to cpd:plan-x:web-tests after compaction.";
    const eventResult = linkifyMentions(eventText, liveKeys);
    const { container: eventContainer, unmount: unmountEvent } = mount(<div>{eventResult}</div>);
    const eventLinks = eventContainer.querySelectorAll('a[data-part="agent-mention"]');
    expect(eventLinks).toHaveLength(1);
    expect(eventLinks[0]?.getAttribute("data-agent-key")).toBe("cpd:plan-x:web-tests");
    unmountEvent();

    // 4. Tool row prose scenario (title or argument summary)
    const toolText = "spawned agent cpd:plan-x for execution";
    const toolResult = linkifyMentions(toolText, liveKeys);
    const { container: toolContainer, unmount: unmountTool } = mount(<div>{toolResult}</div>);
    const toolLinks = toolContainer.querySelectorAll('a[data-part="agent-mention"]');
    expect(toolLinks).toHaveLength(1);
    expect(toolLinks[0]?.getAttribute("data-agent-key")).toBe("cpd:plan-x");
    unmountTool();
  });

  it("leaves non-agent keys and invalid syntax as plain text", () => {
    const liveKeys = new Set(["cpd", "cpd:plan-x"]);

    // Rule 1: adjacent to /, \ or @ (paths, emails, etc.) must not be linkified
    const pathText = "See file at /cpd:plan-x or \\cpd:plan-x or user@cpd:plan-x";
    const pathResult = linkifyMentions(pathText, liveKeys);
    const { container: pathContainer, unmount: unmountPath } = mount(<div>{pathResult}</div>);
    expect(pathContainer.querySelectorAll('a[data-part="agent-mention"]')).toHaveLength(0);
    expect(pathContainer.textContent).toBe(pathText);
    unmountPath();

    // Rule 2: invalid syntax (uppercase, single char context, etc.) must not be linkified
    const invalidSyntaxText = "Invalid keys: CPD, CPD:plan-x, c:p, cpd:plan-x:job-y:extra.";
    const invalidResult = linkifyMentions(invalidSyntaxText, liveKeys);
    const { container: invalidContainer, unmount: unmountInvalid } = mount(<div>{invalidResult}</div>);
    expect(invalidContainer.querySelectorAll('a[data-part="agent-mention"]')).toHaveLength(0);
    expect(invalidContainer.textContent).toBe(invalidSyntaxText);
    unmountInvalid();

    // Rule 3: parses as agent key grammar, but NOT in liveAgentKeys (unknown context or plan)
    // Especially important for ordinary lowercase English words that parse as manager keys
    const nonAgentText = "The word manager or context or plan or cpd:plan-unknown is plain text.";
    const nonAgentResult = linkifyMentions(nonAgentText, liveKeys);
    const { container: nonAgentContainer, unmount: unmountNonAgent } = mount(<div>{nonAgentResult}</div>);
    expect(nonAgentContainer.querySelectorAll('a[data-part="agent-mention"]')).toHaveLength(0);
    expect(nonAgentContainer.textContent).toBe(nonAgentText);
    unmountNonAgent();
  });
});
