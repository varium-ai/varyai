import { readFileSync } from "node:fs";
import postcss from "postcss";
import tailwindcss from "tailwindcss";
import { beforeAll, describe, expect, it } from "vitest";

describe("dialog animation CSS", () => {
  let root: postcss.Root;

  beforeAll(async () => {
    const result = await postcss([
      tailwindcss({
        content: [{ raw: "", extension: "html" }],
        corePlugins: { preflight: false },
      }),
    ]).process(readFileSync("src/index.css", "utf8"), {
      from: "src/index.css",
    });
    root = result.root;
  });

  it.each(["vary-dialog-in", "vary-dialog-out"])(
    "emits %s with scale, opacity and preserved centering",
    (name) => {
      const frames = root.nodes.find(
        (node) =>
          node.type === "atrule" &&
          node.name === "keyframes" &&
          node.params === name,
      );
      expect(frames?.toString()).toContain("var(--tw-translate-x, 0)");
      expect(frames?.toString()).toContain("var(--tw-translate-y, 0)");
      expect(frames?.toString()).toContain("scale(0.95)");
      expect(frames?.toString()).toContain("scale(1)");
      expect(frames?.toString()).toContain("opacity: 0");
    },
  );

  it("emits actual entry/exit animations and disables them for reduced motion", () => {
    const css = root.toString();
    expect(css).toContain("animation: vary-dialog-in 220ms");
    expect(css).toContain("animation: vary-dialog-out 160ms");
    expect(css).toContain("animation: vary-dialog-backdrop-out 160ms");
    const media = root.nodes.find(
      (node) =>
        node.type === "atrule" &&
        node.name === "media" &&
        node.params === "(prefers-reduced-motion: reduce)",
    );
    expect(media?.toString()).toContain(".vary-dialog-motion[data-state]");
    expect(media?.toString()).toContain(
      ".vary-dialog-backdrop-motion[data-state]",
    );
    expect(media?.toString()).toContain("animation: none");
  });
});
