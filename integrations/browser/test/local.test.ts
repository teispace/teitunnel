import assert from "node:assert/strict";
import { test } from "node:test";
import { localOrigin } from "../src/local.ts";

test("only pages from this computer or the network can be shared", () => {
  assert.equal(localOrigin("http://localhost:5173/app?x=1"), "http://localhost:5173");
  assert.equal(localOrigin("https://shop.localhost/"), "https://shop.localhost");
  assert.equal(localOrigin("http://192.168.1.20:8000/"), "http://192.168.1.20:8000");
  assert.equal(localOrigin("http://[::1]:3000/"), "http://[::1]:3000");
  for (const page of [
    "https://example.com/",
    "https://8.8.8.8/",
    "file:///etc/passwd",
    "chrome://settings",
    "http://localhost.evil.com/",
    undefined,
    "not a url",
  ]) {
    assert.equal(localOrigin(page), null, String(page));
  }
});
