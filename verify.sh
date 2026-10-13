#!/usr/bin/env bash
set -euo pipefail
npx tsc --noEmit -p . && node --test
