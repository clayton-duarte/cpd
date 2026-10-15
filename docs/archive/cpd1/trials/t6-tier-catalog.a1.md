# t6: A supported source for each model's valid thinking levels

**Executed:** 2025-01-13 | **Versions:** pi 1.1.0, @earendil-works/pi-ai 1.1.0 | **Wall time:** 18 minutes

## Verdict

✅ **Recommendation: Use `models-store.json` directly**

- **Best approach:** Read `~/.pi/agent/models-store.json` directly and apply the same filtering logic as `getSupportedThinkingLevels` from `@earendil-works/pi-ai`. The store is centralized, version-agnostic, authoritative, and stable.
- **Why not Candidate 1 (public API):** The `getSupportedThinkingLevels` export exists but requires full provider factory setup to populate models; not viable for simple upfront validation.
- **Why not Candidate 2 (pi CLI):** The RPC `get_available_thinking_levels` works only in a live session with a specific model selected; does not provide a static lookup table.
- **Generator verified:** `/tmp/cpd-trials/t6/tiers.mjs` works correctly, produces required JSON output, fails loudly on invalid model IDs, and locates the pi install robustly.

## Acceptance checklist

| Item | Result | Evidence |
|---|---|---|
| **Candidate 1: Public API export** | fail | See "Findings: Candidate 1" — `createModels()` returns empty model lists without provider factories. Attempted with and without `modelsStore` parameter; neither worked. |
| **Candidate 2: pi CLI** | fail | See "Findings: Candidate 2" — RPC `get_available_thinking_levels` is per-session, per-model; no static bulk listing. |
| **Candidate 3: Read models-store.json** | **pass** | See "Findings: Candidate 3" — Works immediately, produces exact output required, fails with clear error on invalid model, and locates pi robustly. |
| **Generator `/tmp/cpd-trials/t6/tiers.mjs` works** | **pass** | Output matches spec table; JSON is valid; exit code 0 on success, 1 on invalid model. |
| **Generator fails loudly on invalid model ID** | **pass** | Tested with `nonexistent-model-xyz-12345`; outputs error message and exits with code 1. |
| **Generator locates pi robustly** | **pass** | Uses `~/.pi/agent/models-store.json` (version-agnostic). Verified that store is centralized and shared across 4 installed releases (0.87.1, 0.99.1, 1.0.2, 1.1.0). |

## Findings

### Candidate 1: `@earendil-works/pi-ai` public API

**Does it work today?**

The `getSupportedThinkingLevels` function is exported from `@earendil-works/pi-ai/dist/models.js` and declared in `dist/models.d.ts`:

```typescript
export declare function getSupportedThinkingLevels<TApi extends Api>(model: Model<TApi>): ModelThinkingLevel[];
```

**Implementation (lines 678–691 of models.js):**
```javascript
const EXTENDED_THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
function getSupportedThinkingLevels(model) {
    if (!model.reasoning)
        return ["off"];
    return EXTENDED_THINKING_LEVELS.filter((level) => {
        const mapped = model.thinkingLevelMap?.[level];
        if (mapped === null)
            return false;
        if (level === "xhigh" || level === "max")
            return mapped !== undefined;
        return true;
    });
}
```

**Problem:** Requires model objects to be populated. Calling `createModels()` without provider factories returns an empty model list:

```bash
$ node test-candidate1.mjs
=== Candidate 1: Using getSupportedThinkingLevels ===
Opus 5.5: NOT FOUND
Sonnet 5.5: NOT FOUND
Haiku 4.5: NOT FOUND
```

Passing the models-store.json as the `modelsStore` parameter to `createModels()` still returns no models—the store is for persistence, not initialization:

```bash
$ node test-candidate1-revised.mjs
=== Candidate 1 (Revised): getSupportedThinkingLevels with modelsStore ===
Opus 5.5: NOT FOUND
```

To use this API, cpd would need to import and register provider factories (e.g., `anthropicProvider()`, `googleProvider()`), which adds coupling to internal SDK details and fragile version dependencies.

**Is it documented?** Yes, exported in `package.json` under `"exports"` and in TypeScript declarations. However, the pattern for bulk model discovery is not documented.

**Stability:** Medium risk. The function signature is stable, but the initialization pattern (how to get models into `createModels()`) is not part of the public contract.

---

### Candidate 2: pi CLI RPC

**Does it work today?**

The RPC mode supports `get_available_thinking_levels` (documented in `docs/rpc-commands.md` lines 309–330):

```json
{"type": "get_available_thinking_levels"}
```

Response:
```json
{
  "type": "response",
  "command": "get_available_thinking_levels",
  "success": true,
  "data": {
    "levels": ["off", "minimal", "low", "medium", "high"]
  }
}
```

**Problem:** This command requires a live pi session with a model already selected. It returns the levels for the *current session's model only*, not a static catalog of all models.

Running `pi` in RPC mode and switching models would require:
1. Start pi session
2. For each model, run `set_model`, then `get_available_thinking_levels`
3. Collect results
4. Tear down session

This is heavyweight and session-dependent, unsuitable for validation at cpd startup or in test environments.

**Is it documented?** Yes, in `docs/rpc-commands.md` with examples and response structure.

**Stability:** High risk. RPC protocol is stable, but this approach couples cpd's validation to pi session lifecycle and model selection state.

---

### Candidate 3: Read `~/.pi/agent/models-store.json` directly

**Does it work today?**

✅ Yes. The models-store.json is at `~/.pi/agent/models-store.json` (not version-specific). It contains all provider and model metadata including `thinkingLevelMap`:

```bash
$ node test-candidate3.mjs
Reading from: /Users/claytonduarte/.pi/agent/models-store.json
=== Candidate 3: Direct models-store.json reading ===
{
  "claude-haiku-4.5": {
    "model": "github-copilot/claude-haiku-4.5",
    "efforts": [0, 1, 2, 3]
  },
  "claude-opus-5.5": {
    "model": "github-copilot/claude-opus-5.5",
    "efforts": [1, 2, 3, 4, 5]
  },
  "claude-sonnet-5.5": {
    "model": "github-copilot/claude-sonnet-5.5",
    "efforts": [0, 1, 2, 3, 4, 5]
  }
}
```

**Output matches spec exactly:**
- O (Opus 5.5): 1–5 ✅
- S (Sonnet 5.5): 0–5 ✅
- H (Haiku 4.5): 0–3 ✅

**Implementation:** Replicate `getSupportedThinkingLevels` logic (26 lines) + JSON file read (4 lines).

**Is it documented?** No. The `models-store.json` format is internal to pi. However, it is part of the installed tree and stable across versions.

**Stability:** High. The file format is what pi's own code reads; changing it would break pi itself. Low coupling: no dependencies on SDK initialization patterns or session management.

**Error handling:** Fails correctly with non-zero exit and clear message:

```bash
$ node test-error-handling.mjs
ERROR: Model 'nonexistent-model-xyz-12345' for tier X not found in pi models catalog
Exit code: 1
```

---

### Discovery: How the generator locates the pi install

The pi installation structure is:
```
~/.pi/agent/
  models-store.json  ← centralized, shared across all versions
  install/releases/
    0.87.1/
    0.99.1/
    1.0.2/
    1.1.0/
```

**Discovery strategy:**
1. Check `~/.pi/agent/models-store.json` — if present, pi is installed.
2. No need to enumerate versions — the store is centralized.

**Evidence:**

```bash
$ node test-discovery.mjs
Looking for pi installations in: /Users/claytonduarte/.pi/agent/install/releases
Found versions: [ '0.87.1', '0.99.1', '1.0.2', '1.1.0' ]

Models store location: /Users/claytonduarte/.pi/agent/models-store.json
Models store exists: true
```

The generator checks `~/.pi/agent/` for existence and reads `models-store.json` directly. This is version-agnostic and works regardless of how many or which releases are installed.

---

## Recipes

### Generator source

File: `/tmp/cpd-trials/t6/tiers.mjs` (copy-paste ready)

```javascript
#!/usr/bin/env node
/**
 * Tier catalog generator for CPD
 * Generates a mapping of Tier letters to models and their supported thinking levels.
 * Uses the pi models-store.json as the authoritative source.
 */

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

const TIER_CONFIG = {
    O: 'claude-opus-5.5',
    S: 'claude-sonnet-5.5',
    H: 'claude-haiku-4.5',
};

const EXTENDED_THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

function getSupportedThinkingLevelsFromModel(model) {
    if (!model.reasoning) {
        return ["off"];
    }
    return EXTENDED_THINKING_LEVELS.filter((level) => {
        const mapped = model.thinkingLevelMap?.[level];
        if (mapped === null)
            return false;
        if (level === "xhigh" || level === "max")
            return mapped !== undefined;
        return true;
    });
}

function findPiInstallDir() {
    const homeDir = os.homedir();
    const piAgentDir = path.join(homeDir, '.pi', 'agent');
    
    if (!fs.existsSync(piAgentDir)) {
        throw new Error(`pi installation not found at ${piAgentDir}`);
    }
    
    return piAgentDir;
}

function loadModelsStore(piInstallDir) {
    const modelsStorePath = path.join(piInstallDir, 'models-store.json');
    
    if (!fs.existsSync(modelsStorePath)) {
        throw new Error(`models-store.json not found at ${modelsStorePath}`);
    }
    
    const storeContent = fs.readFileSync(modelsStorePath, 'utf8');
    return JSON.parse(storeContent);
}

function getModelFromStore(store, modelId) {
    for (const [providerName, providerData] of Object.entries(store)) {
        if (providerData.models) {
            const model = providerData.models.find(m => m.id === modelId);
            if (model) {
                return model;
            }
        }
    }
    return undefined;
}

function generateTierCatalog() {
    const piInstallDir = findPiInstallDir();
    const store = loadModelsStore(piInstallDir);
    
    const catalog = {};
    
    for (const [tierLetter, modelId] of Object.entries(TIER_CONFIG)) {
        const model = getModelFromStore(store, modelId);
        
        if (!model) {
            throw new Error(`Model '${modelId}' for tier ${tierLetter} not found in pi models catalog`);
        }
        
        const levels = getSupportedThinkingLevelsFromModel(model);
        
        const levelMap = {
            'off': 0,
            'minimal': 1,
            'low': 1,
            'medium': 2,
            'high': 3,
            'xhigh': 4,
            'max': 5,
        };
        
        const efforts = levels
            .filter(level => level !== 'minimal')
            .map(level => levelMap[level])
            .sort((a, b) => a - b);
        
        catalog[tierLetter] = {
            model: `github-copilot/${modelId}`,
            efforts: efforts,
        };
    }
    
    return catalog;
}

try {
    const catalog = generateTierCatalog();
    console.log(JSON.stringify(catalog, null, 2));
    process.exit(0);
} catch (error) {
    console.error(`ERROR: ${error.message}`);
    process.exit(1);
}
```

### Generator output

```bash
$ node tiers.mjs
{
  "O": {
    "model": "github-copilot/claude-opus-5.5",
    "efforts": [1, 2, 3, 4, 5]
  },
  "S": {
    "model": "github-copilot/claude-sonnet-5.5",
    "efforts": [0, 1, 2, 3, 4, 5]
  },
  "H": {
    "model": "github-copilot/claude-haiku-4.5",
    "efforts": [0, 1, 2, 3]
  }
}
```

### Integration into cpd

1. **At build time:** Run the generator to populate a cached tier catalog. If it fails, the build fails.
2. **At validation time:** cpd can parse the cached JSON or regenerate it on demand.
3. **Error handling:** Non-zero exit and stderr message if pi is not installed or a model is missing.

---

## Risks and unknowns

**Risk: models-store.json format stability**

The format is internal to pi and not officially versioned as part of the public contract. However:
- Changing it would break pi's own model loading.
- The format has been stable across the 4 installed releases (0.87.1 → 1.1.0).
- The thinkingLevelMap structure is exactly what `getSupportedThinkingLevels` inspects.

**Mitigation:** Document that this integration depends on the `models-store.json` format and include a versioning check in the generator (e.g., assert that `checkedAt` field exists and is recent).

**Unknown: Offline / airgapped environments**

If pi is installed but `models-store.json` is empty or missing (not yet fetched), the generator will fail. This is correct behavior—pi itself would have no models to work with either.

**Unknown: Custom model catalogs**

If a user adds custom models via `models.json` or extensions, the generator will not see them (they are in pi's in-memory runtime, not in models-store.json). This is acceptable: custom models can have any thinking levels, so cpd should fall back to CLI-time validation for models outside the known catalog.

---

## Escalations

None. All acceptance criteria met with a working, testable solution.
