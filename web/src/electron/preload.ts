import { contextBridge, ipcRenderer } from "electron";
import { createBridge } from "./bridge.ts";

contextBridge.exposeInMainWorld("cpdElectron", createBridge(ipcRenderer));
