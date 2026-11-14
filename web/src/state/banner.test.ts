import { expect, it } from "vitest";
import { engineBanner } from "./banner";

const base = {
  connection: "open" as const,
  engineRunning: true,
  lastSeenAt: null,
};

it("shows nothing when connected and the engine runs", () => {
  expect(engineBanner(base)).toBeNull();
});
it("shows engine-down since engineDownSince", () => {
  expect(
    engineBanner({
      ...base,
      engineRunning: false,
      engineDownSince: "2026-01-01T00:00:00Z",
    }),
  ).toEqual({
    kind: "engine-down",
    since: "2026-01-01T00:00:00Z",
  });
});
it("shows engine-restarting when the reason is install", () => {
  expect(
    engineBanner({
      ...base,
      engineRunning: false,
      engineDownSince: "2026-01-01T00:00:00Z",
      engineRestartReason: "install",
    }),
  ).toEqual({ kind: "engine-restarting", since: "2026-01-01T00:00:00Z" });
});
it("shows server-unreachable since last seen when the socket is closed", () => {
  expect(
    engineBanner({
      ...base,
      connection: "closed",
      lastSeenAt: "2026-01-02T00:00:00Z",
    }),
  ).toEqual({
    kind: "server-unreachable",
    since: "2026-01-02T00:00:00Z",
  });
});
it("shows reconnecting while the socket reconnects", () => {
  expect(engineBanner({ ...base, connection: "reconnecting" })?.kind).toBe(
    "reconnecting",
  );
});
it("clears when the engine is back", () => {
  const down = {
    ...base,
    engineRunning: false,
    engineDownSince: "2026-01-01T00:00:00Z",
  };
  expect(engineBanner(down)).not.toBeNull();
  expect(
    engineBanner({ ...down, engineRunning: true, engineDownSince: undefined }),
  ).toBeNull();
});
