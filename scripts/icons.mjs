// Regenerates every app icon and the installer images from the SVG masters.
//
//   pnpm icons
//
// - `src-tauri/icons/icon.svg` (64 px and up) → all the PNGs, `icon.icns` and the 64 and 256 px
//   entries of `icon.ico`, through `tauri icon`.
// - `src-tauri/icons/icon-<size>.svg` (16, 20, 24, 32, 48) → the `icon.ico` entry of that size
//   (system tray, title bar, taskbar, Start menu, desktop) and, for 32, `32x32.png`. They are the
//   same mark drawn on that size's pixel grid, so the bars stay crisp; 16 to 24 px keep five bars.
// - `src-tauri/icons/nsis-header.bmp` (150×57, with the 48 px icon) and `nsis-sidebar.bmp`
//   (164×314) for the installer pages.
//
// Only Node and the Tauri CLI are needed: the PNGs `tauri icon` writes are decoded here and the
// ICO and BMP files are written by hand.

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const icons = join(root, "src-tauri", "icons");
const work = join(root, "src-tauri", "target", "icons");
const tauriCli = createRequire(import.meta.url).resolve(
  "@tauri-apps/cli/tauri.js",
);

/** Sizes of `icon.ico` drawn from their own pixel-hinted master; the rest come from icon.svg. */
const HINTED_SIZES = [16, 20, 24, 32, 48];
const LARGE_SIZES = [64, 256];

/** Ruta colors used around the icon in the installer images. */
const NAVY = [0x12, 0x26, 0x3a]; // dark --color-surface, the icon's tile
const WHITE = [0xff, 0xff, 0xff]; // the installer's header background

function tauriIcon(input, args = []) {
  execFileSync(process.execPath, [tauriCli, "icon", input, ...args], {
    cwd: root,
    stdio: ["ignore", "ignore", "inherit"],
  });
}

/** Renders `svg` at each size; returns { size: pngBytes }. */
function renderPngs(svg, sizes) {
  const out = join(work, String(Math.random()).slice(2));
  tauriIcon(svg, ["-o", out, ...sizes.flatMap((s) => ["-p", String(s)])]);
  return Object.fromEntries(
    sizes.map((s) => [s, readFileSync(join(out, `${s}x${s}.png`))]),
  );
}

/** Decodes an 8-bit RGBA, non-interlaced PNG (what `tauri icon` writes). */
function decodePng(png) {
  let pos = 8;
  let header;
  const data = [];
  while (pos < png.length) {
    const length = png.readUInt32BE(pos);
    const type = png.toString("ascii", pos + 4, pos + 8);
    const chunk = png.subarray(pos + 8, pos + 8 + length);
    if (type === "IHDR") {
      header = {
        width: chunk.readUInt32BE(0),
        height: chunk.readUInt32BE(4),
        depth: chunk[8],
        color: chunk[9],
        interlace: chunk[12],
      };
    } else if (type === "IDAT") {
      data.push(chunk);
    } else if (type === "IEND") {
      break;
    }
    pos += 12 + length;
  }
  const { width, height, depth, color, interlace } = header;
  if (depth !== 8 || color !== 6 || interlace !== 0) {
    throw new Error(`unsupported PNG: depth ${depth}, color ${color}`);
  }
  const raw = inflateSync(Buffer.concat(data));
  const stride = width * 4;
  const pixels = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = y * (stride + 1) + 1;
    for (let x = 0; x < stride; x++) {
      const a = x >= 4 ? pixels[y * stride + x - 4] : 0;
      const b = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const c = x >= 4 && y > 0 ? pixels[(y - 1) * stride + x - 4] : 0;
      let predictor = 0;
      if (filter === 1) predictor = a;
      else if (filter === 2) predictor = b;
      else if (filter === 3) predictor = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const [pa, pb, pc] = [
          Math.abs(p - a),
          Math.abs(p - b),
          Math.abs(p - c),
        ];
        predictor = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      pixels[y * stride + x] = (raw[line + x] + predictor) & 0xff;
    }
  }
  return { width, height, pixels };
}

/** An ICO with PNG-compressed entries (Windows Vista and later). */
function encodeIco(pngs) {
  const sizes = Object.keys(pngs)
    .map(Number)
    .sort((a, b) => a - b);
  const header = Buffer.alloc(6 + 16 * sizes.length);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(sizes.length, 4);
  let offset = header.length;
  sizes.forEach((size, i) => {
    const entry = 6 + 16 * i;
    header[entry] = size >= 256 ? 0 : size;
    header[entry + 1] = size >= 256 ? 0 : size;
    header.writeUInt16LE(1, entry + 4); // planes
    header.writeUInt16LE(32, entry + 6); // bits per pixel
    header.writeUInt32LE(pngs[size].length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += pngs[size].length;
  });
  return Buffer.concat([header, ...sizes.map((s) => pngs[s])]);
}

/** A 24-bit BMP of `width`×`height` filled with `background`, with `icon` drawn at (x, y). */
function encodeBmp(width, height, background, icon, x, y) {
  const rowSize = Math.ceil((width * 3) / 4) * 4;
  const bmp = Buffer.alloc(54 + rowSize * height);
  bmp.write("BM", 0, "ascii");
  bmp.writeUInt32LE(bmp.length, 2);
  bmp.writeUInt32LE(54, 10);
  bmp.writeUInt32LE(40, 14);
  bmp.writeInt32LE(width, 18);
  bmp.writeInt32LE(height, 22);
  bmp.writeUInt16LE(1, 26);
  bmp.writeUInt16LE(24, 28);
  bmp.writeUInt32LE(rowSize * height, 34);
  bmp.writeInt32LE(2835, 38); // 72 dpi
  bmp.writeInt32LE(2835, 42);
  for (let row = 0; row < height; row++) {
    for (let col = 0; col < width; col++) {
      let rgb = background;
      const ix = col - x;
      const iy = row - y;
      if (ix >= 0 && iy >= 0 && ix < icon.width && iy < icon.height) {
        const p = (iy * icon.width + ix) * 4;
        const alpha = icon.pixels[p + 3] / 255;
        rgb = background.map((bg, i) =>
          Math.round(icon.pixels[p + i] * alpha + bg * (1 - alpha)),
        );
      }
      // Rows are stored bottom-up, pixels as BGR.
      const at = 54 + (height - 1 - row) * rowSize + col * 3;
      bmp[at] = rgb[2];
      bmp[at + 1] = rgb[1];
      bmp[at + 2] = rgb[0];
    }
  }
  return bmp;
}

/**
 * Rewrites an ICNS with its entries sorted by type. `tauri icon` writes them in a random order,
 * so without this every run changes `icon.icns` even when the drawing did not change.
 */
function sortIcns(file) {
  const icns = readFileSync(file);
  const entries = [];
  for (let pos = 8; pos < icns.length; pos += icns.readUInt32BE(pos + 4)) {
    entries.push(icns.subarray(pos, pos + icns.readUInt32BE(pos + 4)));
  }
  entries.sort((a, b) => Buffer.compare(a.subarray(0, 4), b.subarray(0, 4)));
  writeFileSync(file, Buffer.concat([icns.subarray(0, 8), ...entries]));
}

rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });

// Every default size (PNGs, ICNS, a first ICO) from the full master, then drop the mobile
// icons: Windows is the only target.
tauriIcon("src-tauri/icons/icon.svg");
for (const mobile of ["android", "ios"]) {
  rmSync(join(icons, mobile), { recursive: true, force: true });
}
sortIcns(join(icons, "icon.icns"));

const hinted = Object.fromEntries(
  HINTED_SIZES.map((s) => [
    s,
    renderPngs(`src-tauri/icons/icon-${s}.svg`, [s])[s],
  ]),
);
const large = renderPngs("src-tauri/icons/icon.svg", [...LARGE_SIZES, 96]);
writeFileSync(
  join(icons, "icon.ico"),
  encodeIco({
    ...hinted,
    ...Object.fromEntries(LARGE_SIZES.map((s) => [s, large[s]])),
  }),
);
// The 32 px PNG is the first icon in tauri.conf.json: use the hinted drawing there too.
writeFileSync(join(icons, "32x32.png"), hinted[32]);

// Header: the 48 px icon at the right of the white header band.
const headerIcon = decodePng(hinted[48]);
writeFileSync(
  join(icons, "nsis-header.bmp"),
  encodeBmp(
    150,
    57,
    WHITE,
    headerIcon,
    150 - 48 - 8,
    Math.floor((57 - 48) / 2),
  ),
);
// Sidebar (welcome and finish pages): the 96 px icon on the tile's navy, near the top.
const sidebarIcon = decodePng(large[96]);
writeFileSync(
  join(icons, "nsis-sidebar.bmp"),
  encodeBmp(164, 314, NAVY, sidebarIcon, (164 - 96) / 2, 40),
);

rmSync(work, { recursive: true, force: true });
console.log("Icons written to src-tauri/icons");
