import { describe, expect, it } from "vitest";
import { isWebUrl } from "./markdown";

describe("web URLs", () => {
  it("accepts HTTP and HTTPS links", () => {
    expect(isWebUrl("https://example.com/docs")).toBe(true);
    expect(isWebUrl("http://localhost:3000")).toBe(true);
  });

  it("rejects non-web and relative links", () => {
    expect(isWebUrl("mailto:hello@example.com")).toBe(false);
    expect(isWebUrl("/docs/getting-started")).toBe(false);
    expect(isWebUrl("not a URL")).toBe(false);
  });
});
