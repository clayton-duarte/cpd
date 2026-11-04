import type { UiMode } from "./channels.ts";
import { createExternalLinkHandler, resolveAssetPaths, setupNativeShell, type ImageLike, type NativeShellDeps } from "./native.ts";

type Template = { label?: string; type?: "separator"; click?: () => void }[];
type FakeImage = ImageLike & { path: string; template: boolean | null };

function harness(platform: NodeJS.Platform, focused = false) {
  const calls: string[] = [];
  const modes: UiMode[] = [];
  const trayImages: FakeImage[] = [];
  const bounces: string[] = [];
  const badges: string[] = [];
  const icons: string[] = [];
  const about: { applicationName: string; applicationVersion: string; credits: string }[] = [];
  const nativeTheme: { themeSource: "system" | "light" | "dark" } = { themeSource: "system" };
  const created: FakeImage[] = [];
  const tray = {
    setImage: (i: ImageLike) => { trayImages.push(i as FakeImage); },
    setToolTip: (t: string) => { calls.push(`tooltip:${t}`); },
    setContextMenu: (m: unknown) => { calls.push("menu"); menus.push(m as Template); },
    destroy: () => { calls.push("destroy"); },
  };
  const menus: Template[] = [];
  const trayCreatedWith: FakeImage[] = [];
  const state = { focused };
  const deps: NativeShellDeps = {
    platform,
    app: {
      dock: {
        setIcon: (p) => { icons.push(p); },
        setBadge: (t) => { badges.push(t); },
        bounce: (t) => { bounces.push(t); return 1; },
      },
      setAboutPanelOptions: (o) => { about.push(o); },
      getVersion: () => "9.9.9",
    },
    nativeTheme,
    nativeImage: {
      createFromPath: (path) => {
        const img: FakeImage = { path, template: null, setTemplateImage(t) { img.template = t; } };
        created.push(img);
        calls.push(`image:${path}`);
        return img;
      },
    },
    createTray: (i) => { trayCreatedWith.push(i as FakeImage); calls.push("createTray"); return tray; },
    buildMenu: (t) => { calls.push("buildMenu"); return t; },
    isWindowFocused: () => state.focused,
    assets: { icon: "/a/icon.png", tray: "/a/tray.png", trayAttention: "/a/trayAttention.png" },
    actions: {
      openCockpit: () => { calls.push("openCockpit"); },
      setMode: (m) => { modes.push(m); },
      quit: () => { calls.push("quit"); },
    },
  };
  return { deps, calls, modes, trayImages, bounces, badges, icons, about, nativeTheme, created, menus, trayCreatedWith, state };
}

test("resolveAssetPaths uses build under the app path in dev", () => {
  expect(resolveAssetPaths({ packaged: false, resourcesPath: "/res", appPath: "/app" })).toEqual({
    icon: "/app/build/icon.png",
    tray: "/app/build/tray/trayTemplate.png",
    trayAttention: "/app/build/tray/trayAttentionTemplate.png",
  });
});

test("resolveAssetPaths uses the resources path when packaged", () => {
  expect(resolveAssetPaths({ packaged: true, resourcesPath: "/res", appPath: "/app" })).toEqual({
    icon: "/res/icon.png",
    tray: "/res/tray/trayTemplate.png",
    trayAttention: "/res/tray/trayAttentionTemplate.png",
  });
});

test("setupNativeShell forces the dark theme and sets the About panel on every platform", () => {
  for (const platform of ["darwin", "linux", "win32"] as const) {
    const h = harness(platform);
    setupNativeShell(h.deps);
    expect(h.nativeTheme.themeSource).toBe("dark");
    expect(h.about).toEqual([{ applicationName: "cpd", applicationVersion: "9.9.9", credits: "Continuous Parallel Delivery" }]);
  }
});

test("setupNativeShell sets the Dock icon on darwin", () => {
  const h = harness("darwin");
  setupNativeShell(h.deps);
  expect(h.icons).toEqual(["/a/icon.png"]);
});

test("setupNativeShell creates a template tray with the cpd menu on darwin", () => {
  const h = harness("darwin");
  setupNativeShell(h.deps);
  expect(h.trayCreatedWith).toHaveLength(1);
  expect(h.trayCreatedWith[0]).toMatchObject({ path: "/a/tray.png", template: true });
  expect(h.created.find((i) => i.path === "/a/trayAttention.png")?.template).toBe(true);
  expect(h.calls).toContain("tooltip:cpd");
  expect(h.menus).toHaveLength(1);
  const items = h.menus[0]!;
  expect(items.map((i) => i.type === "separator" ? "-" : i.label)).toEqual(["Open cockpit", "Switch to TUI", "Switch to WebUI", "-", "Quit cpd"]);
});

test("setupNativeShell touches no dock or tray off darwin", () => {
  for (const platform of ["linux", "win32"] as const) {
    const h = harness(platform);
    const shell = setupNativeShell(h.deps);
    shell.setAttention({ blocking: 1, decisions: 1 });
    shell.dispose();
    expect(h.icons).toEqual([]);
    expect(h.badges).toEqual([]);
    expect(h.bounces).toEqual([]);
    expect(h.trayImages).toEqual([]);
    expect(h.created).toEqual([]);
    expect(h.calls).toEqual([]);
  }
});

test("setAttention sets the Dock badge to blocking plus decisions", () => {
  const h = harness("darwin");
  const shell = setupNativeShell(h.deps);
  shell.setAttention({ blocking: 2, decisions: 3 });
  expect(h.badges.at(-1)).toBe("5");
});

test("setAttention clears the Dock badge when nothing needs you", () => {
  const h = harness("darwin");
  const shell = setupNativeShell(h.deps);
  shell.setAttention({ blocking: 1, decisions: 0 });
  shell.setAttention({ blocking: 0, decisions: 0 });
  expect(h.badges.at(-1)).toBe("");
});

test("setAttention switches the tray image to the attention template and back", () => {
  const h = harness("darwin");
  const shell = setupNativeShell(h.deps);
  shell.setAttention({ blocking: 0, decisions: 1 });
  expect(h.trayImages.at(-1)?.path).toBe("/a/trayAttention.png");
  shell.setAttention({ blocking: 0, decisions: 0 });
  expect(h.trayImages.at(-1)?.path).toBe("/a/tray.png");
});

test("setAttention bounces the Dock when blocking rises while unfocused", () => {
  const h = harness("darwin", false);
  const shell = setupNativeShell(h.deps);
  shell.setAttention({ blocking: 0, decisions: 0 });
  shell.setAttention({ blocking: 1, decisions: 0 });
  expect(h.bounces).toEqual(["informational"]);
});

test("setAttention does not bounce when focused, when blocking falls, or on the first call", () => {
  const first = harness("darwin", false);
  setupNativeShell(first.deps).setAttention({ blocking: 3, decisions: 0 });
  expect(first.bounces).toEqual([]);

  const focused = harness("darwin", true);
  const f = setupNativeShell(focused.deps);
  f.setAttention({ blocking: 0, decisions: 0 });
  f.setAttention({ blocking: 2, decisions: 0 });
  expect(focused.bounces).toEqual([]);

  const falling = harness("darwin", false);
  const s = setupNativeShell(falling.deps);
  s.setAttention({ blocking: 2, decisions: 0 });
  s.setAttention({ blocking: 1, decisions: 0 });
  expect(falling.bounces).toEqual([]);
});

test("tray menu items call the actions", () => {
  const h = harness("darwin");
  setupNativeShell(h.deps);
  const items = h.menus[0]!;
  const click = (label: string) => items.find((i) => i.label === label)?.click?.();
  click("Open cockpit");
  click("Switch to TUI");
  click("Switch to WebUI");
  click("Quit cpd");
  expect(h.calls.filter((c) => c === "openCockpit" || c === "quit")).toEqual(["openCockpit", "quit"]);
  expect(h.modes).toEqual(["tui", "webui"]);
});

test("dispose destroys the tray", () => {
  const h = harness("darwin");
  const shell = setupNativeShell(h.deps);
  shell.dispose();
  shell.dispose();
  expect(h.calls.filter((c) => c === "destroy")).toEqual(["destroy"]);
});

test("createExternalLinkHandler opens the url externally and denies the window", () => {
  const opened: string[] = [];
  const handler = createExternalLinkHandler((url) => opened.push(url));
  expect(handler({ url: "https://example.com/x" })).toEqual({ action: "deny" });
  expect(opened).toEqual(["https://example.com/x"]);
});
