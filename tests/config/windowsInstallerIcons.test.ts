import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { inflateSync } from "node:zlib";
import { describe, expect, it } from "vitest";

const repository = process.cwd();
const tauriDir = path.join(repository, "src-tauri");
const config = JSON.parse(
  readFileSync(path.join(tauriDir, "tauri.conf.json"), "utf8"),
) as {
  bundle?: {
    windows?: {
      nsis?: { installerIcon?: string; uninstallerIcon?: string };
    };
  };
};

function paeth(left: number, up: number, upLeft: number) {
  const predicted = left + up - upLeft;
  const distances = [
    Math.abs(predicted - left),
    Math.abs(predicted - up),
    Math.abs(predicted - upLeft),
  ];
  if (distances[0] <= distances[1] && distances[0] <= distances[2]) {
    return left;
  }
  return distances[1] <= distances[2] ? up : upLeft;
}

function decodePngRgba(png: Buffer) {
  expect(png.subarray(1, 4).toString("ascii")).toBe("PNG");
  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idat: Buffer[] = [];
  while (offset + 12 <= png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.subarray(offset + 4, offset + 8).toString("ascii");
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
    } else if (type === "IDAT") {
      idat.push(data);
    } else if (type === "IEND") {
      break;
    }
    offset += 12 + length;
  }
  expect(bitDepth).toBe(8);
  expect(colorType).toBe(6);

  const stride = width * 4;
  const raw = inflateSync(Buffer.concat(idat));
  const pixels = Buffer.alloc(height * stride);
  let cursor = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[cursor];
    cursor += 1;
    for (let x = 0; x < stride; x += 1) {
      const value = raw[cursor + x];
      const left = x >= 4 ? pixels[y * stride + x - 4] : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const upLeft = x >= 4 && y > 0 ? pixels[(y - 1) * stride + x - 4] : 0;
      let decoded = value;
      if (filter === 1) decoded += left;
      else if (filter === 2) decoded += up;
      else if (filter === 3) decoded += Math.floor((left + up) / 2);
      else if (filter === 4) decoded += paeth(left, up, upLeft);
      pixels[y * stride + x] = decoded & 0xff;
    }
    cursor += stride;
  }
  return { width, height, pixels };
}

describe("Windows icons", () => {
  it("uses the application icon for the NSIS installer", () => {
    const nsis = config.bundle?.windows?.nsis;
    expect(nsis?.installerIcon).toBe("icons/icon.ico");
    expect(existsSync(path.join(tauriDir, nsis?.installerIcon ?? ""))).toBe(
      true,
    );
  });

  it("ships a densely filled 32-pixel Windows icon", () => {
    const bytes = readFileSync(path.join(tauriDir, "icons", "icon.ico"));
    expect(bytes.readUInt16LE(0)).toBe(0);
    expect(bytes.readUInt16LE(2)).toBe(1);
    const count = bytes.readUInt16LE(4);
    expect(count).toBeGreaterThan(0);

    let image: Buffer | undefined;
    for (let index = 0; index < count; index += 1) {
      const at = 6 + index * 16;
      if (bytes[at] === 32 && bytes[at + 1] === 32) {
        const size = bytes.readUInt32LE(at + 8);
        const offset = bytes.readUInt32LE(at + 12);
        image = bytes.subarray(offset, offset + size);
        break;
      }
    }
    expect(image).toBeDefined();

    const { width, height, pixels } = decodePngRgba(image!);
    expect(width).toBe(32);
    expect(height).toBe(32);

    let minX = width;
    let minY = height;
    let maxX = -1;
    let maxY = -1;
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        if (pixels[(y * width + x) * 4 + 3] <= 8) continue;
        minX = Math.min(minX, x);
        minY = Math.min(minY, y);
        maxX = Math.max(maxX, x);
        maxY = Math.max(maxY, y);
      }
    }

    expect(maxX - minX + 1).toBeGreaterThanOrEqual(Math.floor(width * 0.9));
    expect(maxY - minY + 1).toBeGreaterThanOrEqual(Math.floor(height * 0.9));
  });
});
