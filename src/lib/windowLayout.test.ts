import { describe, expect, it } from "vitest";
import { windowLayout } from "./windowLayout";

describe("windowLayout", () => {
  it("places the macOS navigation alongside the native window controls", () => {
    expect(windowLayout(true, false, false)).toEqual({
      dragBarHeight: 0,
      contentTop: 64,
      macTitlebar: true,
    });
    expect(windowLayout(true, false, true).contentTop).toBe(128);
  });

  it("keeps the separate custom controls area on Linux", () => {
    expect(windowLayout(false, true, false)).toEqual({
      dragBarHeight: 32,
      contentTop: 96,
      macTitlebar: false,
    });
  });
});
