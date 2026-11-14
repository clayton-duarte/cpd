// @vitest-environment jsdom
import { act } from "react";
import { describe, expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ResumeButton } from "./ResumeButton.tsx";
import type { Cpd } from "../../server/cockpit/cli.ts";

function fakeCpd(reply: (args: readonly string[]) => Promise<Record<string, unknown>> = async () => ({ ok: true })): { cpd: Cpd; calls: (readonly string[])[] } {
  const calls: (readonly string[])[] = [];
  return { calls, cpd: { json: async (args) => { calls.push(args); return reply(args); } } };
}

describe("ResumeButton", () => {
  it("shows Resume only when the session is stopped", () => {
    const { cpd } = fakeCpd();

    // Not rendered when running / idle
    const idle = mount(<ResumeButton sessionKey="s1" state="idle" cpd={cpd} />);
    expect(idle.container.querySelector('button[data-action="resume"]')).toBeNull();
    idle.unmount();

    // Not rendered when working
    const working = mount(<ResumeButton sessionKey="s1" state="working" cpd={cpd} />);
    expect(working.container.querySelector('button[data-action="resume"]')).toBeNull();
    working.unmount();

    // Not rendered when blocked
    const blocked = mount(<ResumeButton sessionKey="s1" state="blocked" cpd={cpd} />);
    expect(blocked.container.querySelector('button[data-action="resume"]')).toBeNull();
    blocked.unmount();

    // Not rendered when state is undefined / not passed
    const undef = mount(<ResumeButton sessionKey="s1" cpd={cpd} />);
    expect(undef.container.querySelector('button[data-action="resume"]')).toBeNull();
    undef.unmount();

    // Rendered ONLY when state is stopped
    const stopped = mount(<ResumeButton sessionKey="s1" state="stopped" cpd={cpd} />);
    const btn = stopped.container.querySelector('button[data-action="resume"]');
    expect(btn).not.toBeNull();
    expect(btn?.textContent).toBe("Resume");
    stopped.unmount();
  });

  it("click runs cpd session resume for the session key", async () => {
    const { cpd, calls } = fakeCpd(async () => ({ ok: true, session: "manager-session", pane: "%41" }));
    const m = mount(<ResumeButton sessionKey="manager-session" state="stopped" cpd={cpd} />);
    const btn = m.container.querySelector<HTMLButtonElement>('button[data-action="resume"]');
    expect(btn).not.toBeNull();

    await act(async () => {
      btn?.click();
    });

    expect(calls).toEqual([["session", "resume", "manager-session"]]);
    m.unmount();
  });

  it("shows the error when resume fails", async () => {
    const { cpd } = fakeCpd(async () => {
      throw new Error("The cockpit (tmux session \"cpd\" with its main window and chat slot) does not exist: run cpd watch.");
    });
    const m = mount(<ResumeButton sessionKey="s1" state="stopped" cpd={cpd} />);
    const btn = m.container.querySelector<HTMLButtonElement>('button[data-action="resume"]');
    expect(btn).not.toBeNull();

    await act(async () => {
      btn?.click();
    });

    const err = m.container.querySelector('[data-part="resume-error"]');
    expect(err).not.toBeNull();
    expect(err?.textContent).toContain("The cockpit (tmux session \"cpd\" with its main window and chat slot) does not exist: run cpd watch.");
    m.unmount();
  });
});
