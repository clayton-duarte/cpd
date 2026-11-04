// electron-builder afterPack hook.
// macOS 26+ shows a native (Liquid Glass) icon only from an asset catalog.
// electron-builder doesn't compile Icon Composer `.icon` files yet, so
// scripts/prepare-macos-assets.sh compiles build/cpd.icon → build/Assets.car,
// and this hook copies it into the app. `mac.extendInfo.CFBundleIconName: cpd`
// points at it; older macOS keeps using icon.icns.
const fs = require('node:fs');
const path = require('node:path');

exports.default = async function afterPack(ctx) {
  if (ctx.electronPlatformName !== 'darwin') return;

  const source = path.join(__dirname, 'Assets.car');
  if (!fs.existsSync(source)) {
    console.warn('[cpd] build/Assets.car not found; shipping icon.icns only (macOS 26 may frame it in a gray squircle).');
    return;
  }

  const app = `${ctx.packager.appInfo.productFilename}.app`;
  const target = path.join(ctx.appOutDir, app, 'Contents', 'Resources', 'Assets.car');
  fs.copyFileSync(source, target);
  console.log(`[cpd] Assets.car → ${target}`);
};
