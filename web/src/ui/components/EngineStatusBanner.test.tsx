// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { EngineStatusBanner } from "./EngineStatusBanner.tsx";

const now = new Date("2026-01-15T12:05:00Z");

describe("EngineStatusBanner", () => {
  it("shows down since while engine is down", () => {
    const m = mount(<EngineStatusBanner engineRunning={false} engineDownSince="2026-01-15T12:00:00Z" engineRestartReason={null} now={now} />);
    const alert = m.container.querySelector("[role=alert]");
    expect(alert, "banner").not.toBeNull();
    expect(alert?.textContent).toMatch(/engine.*down/i);
    expect(alert?.textContent).toContain("5 minutes");
    m.unmount();
  });

  it("shows install restarting message", () => {
    const m = mount(<EngineStatusBanner engineRunning={false} engineDownSince="2026-01-15T12:00:00Z" engineRestartReason="install" now={now} />);
    const alert = m.container.querySelector("[role=alert]");
    expect(alert, "banner").not.toBeNull();
    expect(alert?.textContent).toMatch(/restarting/i);
    expect(alert?.textContent).toMatch(/install/i);
    m.unmount();
  });

  it("hidden while engine runs", () => {
    const m = mount(<EngineStatusBanner engineRunning engineDownSince={null} engineRestartReason={null} now={now} />);
    expect(m.container.querySelector("[role=alert]")).toBeNull();
    expect(m.container.textContent).toBe("");
    m.unmount();
  });
});
