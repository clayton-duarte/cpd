import { FitAddon, Ghostty, Terminal } from "@aurigax-ai/ghostty-web";
import { DANGO_THEME, TERMINAL_FONT, fitToContainer, startTabTerminal } from "./terminal.ts";
import type { TabTerminal, TerminalEngine } from "./terminal.ts";
import { createViewToggle } from "./view.ts";
import { mountWebui } from "./webui.tsx";
import { LOCAL_TAB_ID, buildTabs, statusLabel, tabsForMode } from "./tabs.ts";
import type { UiMode } from "./channels.ts";

function element(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (!found) throw new Error("missing #" + id);
  return found;
}

// HTMLElement.hidden is typed string | boolean; the toggle only writes booleans.
function boolHidden(el: HTMLElement): { hidden: boolean } {
  return {
    get hidden() { return el.hidden === true; },
    set hidden(value: boolean) { el.hidden = value; },
  } as { hidden: boolean };
}

let buttons = 0;
for (const type of ["mousedown", "mouseup", "mousemove"] as const) {
  window.addEventListener(type, (e: MouseEvent) => { buttons = e.buttons; }, true);
}

async function boot(): Promise<void> {
  const bridge = window.cpdElectron;
  const container = element("terminal");
  const webuiRoot = element("webui-root");
  const tabBar = element("tab-bar");

  // D2: the renderer's startup IPC is this promise. The App calls reportHealthy only after the
  // first cockpit.snapshot (web/CONTRACT.md §15.2); passing it the startup promise defers health:ok
  // until every startup IPC here has resolved and reports health:crash (so the watchdog's rollback
  // runs) when one of them rejects.
  const startup = (async (): Promise<void> => {
    const ghostty = await Ghostty.fromBytes(await bridge.readWasm());

    const showTerminal = createViewToggle(boolHidden(container), boolHidden(webuiRoot), boolHidden(tabBar));
    const hosts = (await bridge.listHosts?.()) ?? [];
    const tabs = buildTabs(hosts);
    const panes = new Map<string, { pane: HTMLElement; host: TabTerminal }>();
    let mode: UiMode = await bridge.getMode();
    let activeId = LOCAL_TAB_ID;

    const createEngine = (host: HTMLElement): TerminalEngine => {
      const term = new Terminal({ ghostty, renderer: "webgl", theme: DANGO_THEME, ...TERMINAL_FONT });
      const fit = new FitAddon();
      term.loadAddon(fit);
      return {
        open: () => term.open(host),
        fit: () => fitToContainer(fit, term),
        size: () => ({ cols: term.cols, rows: term.rows }),
        write: (data) => term.write(data),
        focus: () => term.focus(),
        setPaused: (paused) => term.setPaused(paused),
        onData: (cb) => void term.onData(cb),
        onResize: (cb) => void term.onResize(cb),
        setClipboardWriteHandler: (handler) => { term.clipboardWriteHandler = handler; },
        fontSize: () => term.options.fontSize,
        setFontSize: (px) => { term.options.fontSize = px; },
        cellHeight: () => term.renderer?.getMetrics().height ?? 20,
        setKeyHandler: (handler) => term.attachCustomKeyEventHandler(handler),
        setWheelHandler: (handler) => term.attachCustomWheelEventHandler(handler),
        appOwnsWheel: () => term.hasMouseTracking() || (term.wasmTerm?.isAlternateScreen() ?? false),
        wheelStep: (direction, clientX, clientY) => {
          if (!term.hasMouseTracking()) return direction === "up" ? "\x1b[A" : "\x1b[B";
          const metrics = term.renderer?.getMetrics();
          const rect = host.getBoundingClientRect();
          const col = Math.max(1, Math.floor((clientX - rect.left) / (metrics?.width || 1)) + 1);
          const row = Math.max(1, Math.floor((clientY - rect.top) / (metrics?.height || 1)) + 1);
          return "\x1b[<" + (direction === "up" ? 64 : 65) + ";" + col + ";" + row + "M";
        },
        input: (data) => term.input(data, true),
        paste: (text) => term.paste(text),
        dispose: () => term.dispose(),
      };
    };

    const renderBar = (): void => {
      tabBar.replaceChildren(
        ...tabsForMode(tabs, mode).map((tab) => {
          const button = document.createElement("button");
          button.type = "button";
          button.className = "tab" + (tab.id === activeId ? " active" : "");
          button.addEventListener("click", () => selectTab(tab.id));
          const dot = document.createElement("span");
          dot.className = "dot " + tab.status;
          const label = document.createElement("span");
          label.className = "name";
          label.textContent = tab.label;
          button.append(dot, label);
          const state = statusLabel(tab);
          if (state !== "") {
            const status = document.createElement("span");
            status.className = "state";
            status.textContent = state;
            button.append(status);
          }
          return button;
        }),
      );
    };

    const terminalShown = (): boolean => {
      const active = tabs.find((tab) => tab.id === activeId);
      return active?.kind === "remote" || mode === "tui";
    };

    const applyView = (): void => {
      const shown = terminalShown();
      showTerminal(shown, mode);
      for (const [id, entry] of panes) {
        entry.pane.hidden = id !== activeId;
        entry.host.setVisible(id === activeId && shown);
      }
      renderBar();
    };

    // Creates a tab's terminal lazily, on its first selection, and starts its pty once.
    const ensurePane = (tabId: string): { pane: HTMLElement; host: TabTerminal } => {
      const existing = panes.get(tabId);
      if (existing) return existing;
      const pane = document.createElement("div");
      pane.className = "pane";
      pane.hidden = true;
      container.append(pane);
      const entry = {
        pane,
        host: startTabTerminal({
          tabId,
          engine: createEngine(pane),
          bridge,
          clipboard: {
            writeText: (text) => navigator.clipboard.writeText(text),
            readText: () => navigator.clipboard.readText(),
          },
          pointer: { buttons: () => buttons },
          onStatus: (status, code) => {
            const tab = tabs.find((t) => t.id === tabId);
            if (!tab) return;
            tab.status = status;
            tab.code = code;
            renderBar();
          },
          onError: (error) => console.error(error),
        }),
      };
      panes.set(tabId, entry);
      return entry;
    };

    const selectTab = (tabId: string): void => {
      const created = panes.has(tabId);
      activeId = tabId;
      const tab = tabs.find((t) => t.id === tabId);
      const entry = ensurePane(tabId);
      // A tab's pty is created on its first selection; a failed or exited tab restarts on click and
      // nothing reconnects on its own. A tab that already has a session is never restarted here.
      const restart = tab?.status === "failed" || tab?.status === "exited";
      if (!created || restart) void entry.host.start().catch((error: unknown) => console.error(error));
      applyView();
    };

    const observer = new ResizeObserver(() => panes.get(activeId)?.host.fit());
    observer.observe(container);

    bridge.onMode((next) => {
      mode = next;
      applyView();
    });

    await document.fonts?.ready;
    selectTab(LOCAL_TAB_ID);
  })();

  mountWebui(webuiRoot, {
    webSocketUrl: bridge.webSocketUrl,
    reportCrash: bridge.reportCrash,
    reportHealthy: () => bridge.reportHealthy(startup),
  });

  await startup;
}

boot().catch((error: unknown) => console.error(error));
