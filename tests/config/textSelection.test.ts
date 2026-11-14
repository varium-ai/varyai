import { readFileSync } from "node:fs";
import postcss from "postcss";
import tailwindcss from "tailwindcss";
import { beforeAll, describe, expect, it } from "vitest";

describe("desktop text selection CSS", () => {
  let root: postcss.Root;

  beforeAll(async () => {
    const result = await postcss([
      tailwindcss({
        content: [
          {
            raw: '<div class="select-text select-none"></div>',
            extension: "html",
          },
        ],
      }),
    ]).process(readFileSync("src/index.css", "utf8"), {
      from: "src/index.css",
    });
    root = result.root;
  });

  const selectionValues = (selector: string, property: string) => {
    const values: string[] = [];
    root.walkRules((rule) => {
      if (rule.selectors.includes(selector)) {
        rule.walkDecls(property, (decl) => {
          expect(decl.important).toBeFalsy();
          values.push(decl.value);
        });
      }
    });
    return values;
  };

  it.each(["user-select", "-webkit-user-select"])(
    "disables selection on the application body with %s",
    (property) => {
      expect(selectionValues("body", property)).toContain("none");
      expect(selectionValues("*", property)).toEqual([]);
    },
  );

  it.each([
    "input",
    "textarea",
    '[contenteditable=""]',
    '[contenteditable="true"]',
    '[contenteditable="plaintext-only"]',
    ".cm-content",
    "pre",
    "code",
    ".select-text",
  ])("preserves selection in %s on WebKit and WebView2", (selector) => {
    expect(selectionValues(selector, "user-select")).toContain("text");
    expect(selectionValues(selector, "-webkit-user-select")).toContain("text");
  });

  it("does not opt non-editable UI into selection", () => {
    expect(selectionValues('[contenteditable="false"]', "user-select")).toEqual(
      [],
    );
    expect(selectionValues(".select-none", "user-select")).toEqual(["none"]);
  });
});
