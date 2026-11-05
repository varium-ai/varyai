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
    "emits %s with scale and opacity without animating layout translation",
    (name) => {
      const frames = root.nodes.find(
        (node) =>
          node.type === "atrule" &&
          node.name === "keyframes" &&
          node.params === name,
      );
      expect(frames?.toString()).not.toContain("translate");
      expect(frames?.toString()).not.toContain("--tw-translate");
      expect(frames?.toString()).toContain("scale(0.95)");
      expect(frames?.toString()).toContain("scale(1)");
      expect(frames?.toString()).toContain("opacity: 0");
    },
  );

  it("centers through layout without percentage transforms or translations", () => {
    const centering = root.nodes.find(
      (node) =>
        node.type === "rule" && node.selector === ".vary-dialog-centered",
    );
    expect(centering?.toString()).toContain("inset: 0");
    expect(centering?.toString()).toContain("margin: auto");
    expect(centering?.toString()).not.toContain("translate");
    expect(centering?.toString()).not.toContain("transform");
  });

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
