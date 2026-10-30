import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CONFIG_DIR_NAME, SettingsManager } from "@earendil-works/pi-coding-agent";

type Storage = Parameters<typeof SettingsManager.fromStorage>[0];

/**
 * A pi SettingsManager over the settings files of `agentDir` and `cwd` that only reads them. A read must not take pi's write
 * lock: `SettingsManager.create` locks each existing file even to read it, and parallel processes then contend for it.
 */
export function readOnlySettings(cwd: string, agentDir: string): SettingsManager {
	const storage: Storage = {
		withLock(scope, fn) {
			const path = scope === "global" ? join(agentDir, "settings.json") : join(cwd, CONFIG_DIR_NAME, "settings.json");
			fn(existsSync(path) ? readFileSync(path, "utf-8") : undefined);
		},
	};
	return SettingsManager.fromStorage(storage);
}
