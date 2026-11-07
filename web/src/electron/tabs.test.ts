import type { Tab } from "./tabs.ts";
import { LOCAL_TAB_ID, buildTabs, remoteTabId, statusLabel, tabsForMode } from "./tabs.ts";

function tab(overrides: Partial<Tab> = {}): Tab {
  return { id: "ssh:host", label: "host", kind: "remote", status: "connecting", ...overrides };
}

test("buildTabs pins the local tab first then the hosts in order", () => {
  const tabs = buildTabs([{ alias: "homeserver" }, { alias: "work" }]);

  expect(tabs.map((t) => t.id)).toEqual([LOCAL_TAB_ID, "ssh:homeserver", "ssh:work"]);
  expect(tabs.map((t) => t.kind)).toEqual(["local", "remote", "remote"]);
});

test("buildTabs labels a remote tab with its alias and the ssh id", () => {
  const tabs = buildTabs([{ alias: "homeserver" }]);

  expect(tabs[0]).toMatchObject({ id: LOCAL_TAB_ID, label: "local", kind: "local", status: "connecting" });
  expect(tabs[1]).toEqual({
    id: remoteTabId("homeserver"),
    label: "homeserver",
    kind: "remote",
    status: "connecting",
  });
});

test("tabsForMode keeps remote tabs visible in webui mode", () => {
  const tabs = buildTabs([{ alias: "homeserver" }]);

  expect(tabsForMode(tabs, "webui").map((t) => t.id)).toEqual([LOCAL_TAB_ID, "ssh:homeserver"]);
});

test("tabsForMode keeps every tab in tui mode", () => {
  const tabs = buildTabs([{ alias: "homeserver" }]);

  expect(tabsForMode(tabs, "tui").map((t) => t.id)).toEqual([LOCAL_TAB_ID, "ssh:homeserver"]);
});

test("statusLabel names cpd not found for exit 127", () => {
  expect(statusLabel(tab({ label: "homeserver", status: "failed", code: 127 }))).toBe(
    "cpd not found on homeserver",
  );
});

test("statusLabel names the ssh exit code for a failed tab", () => {
  expect(statusLabel(tab({ status: "failed", code: 255 }))).toBe("ssh failed (exit 255)");
  expect(statusLabel(tab({ status: "failed" }))).toBe("ssh failed");
});

test("statusLabel shows connecting and hides live", () => {
  expect(statusLabel(tab({ status: "connecting" }))).toBe("connecting...");
  expect(statusLabel(tab({ status: "live" }))).toBe("");
  expect(statusLabel(tab({ status: "exited", code: 0 }))).toBe("exited (code 0)");
});
