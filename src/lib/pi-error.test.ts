import { describe, expect, it } from "vitest";
import { describePiError } from "./pi-error";

describe("pi error", () => {
  it("reads the sentence out of a provider payload", () => {
    const raw = `402: {"message":"This request requires more credits.","code":402,"metadata":{"limit_source":"openrouter_credits"}}`;

    expect(describePiError(raw)).toEqual({
      label: "HTTP 402",
      message: "This request requires more credits.",
    });
  });

  it("keeps a named provider and status as the label", () => {
    const raw = `OpenAI API error (401): {"message":"Incorrect API key provided.","type":"invalid_request_error"}`;

    expect(describePiError(raw)).toMatchObject({
      label: "OpenAI API error (401)",
      message: "Incorrect API key provided.",
    });
  });

  it("reads a message nested under an error object", () => {
    const raw = `529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`;

    expect(describePiError(raw)).toMatchObject({
      label: "HTTP 529",
      message: "Overloaded",
    });
  });

  it("shows a plain error whole", () => {
    expect(describePiError("terminated")).toEqual({
      label: "Error",
      message: "terminated",
    });
  });

  /** A stack has one line worth reading; the session file keeps the rest. */
  it("summarises a multi-line error by its first line", () => {
    const raw = "TypeError: fetch failed\n    at node:internal/deps/undici";

    expect(describePiError(raw).message).toBe("TypeError: fetch failed");
  });

  it("falls back to the raw text when the payload will not parse", () => {
    const raw = `500 {"message":"truncated payload`;

    expect(describePiError(raw)).toEqual({ label: "Error", message: raw });
  });

  it("clamps a message that would fill the row", () => {
    const message = "x".repeat(600);
    const description = describePiError(`400: {"message":"${message}"}`);

    expect(description.message).toHaveLength(401);
    expect(description.message.endsWith("…")).toBe(true);
  });

  it("describes an empty error rather than showing nothing", () => {
    expect(describePiError("   ").label).toBe("Error");
  });
});
