#!/usr/bin/env node
// Local Douyin frontierSign: argv[2]=X-MS-STUB, argv[3]=fp.data path.
const fs = require("fs");
const vm = require("vm");

const stub = process.argv[2] || "";
const fp = process.argv[3] || "";
if (!stub || !fp) {
  process.stderr.write("usage: frontier_sign.js <X-MS-STUB> <fp.data>\n");
  process.exit(2);
}

let code = fs.readFileSync(fp, "utf8");
code = code.replace(/catvm\.print\.getAll\(\);/g, "");
code = code.replace(/debugger;/g, "");

const noop = () => {};
const sandbox = {
  console: {
    log: noop, warn: noop, error: noop, info: noop, debug: noop,
    table: noop, dir: noop, time: noop, timeEnd: noop,
  },
  setTimeout, clearTimeout, setInterval, clearInterval,
  Date, Math, JSON, Number, String, Boolean, Array, Object, RegExp,
  Error, TypeError, Map, Set, WeakMap, WeakSet, Promise, Symbol,
  Reflect, Proxy, parseInt, parseFloat, isNaN, encodeURIComponent,
  decodeURIComponent, undefined,
};
sandbox.globalThis = sandbox;
sandbox.window = sandbox;
sandbox.self = sandbox;
sandbox.top = sandbox;
sandbox.parent = sandbox;

vm.runInNewContext(code, sandbox, { timeout: 20000 });
const ac = sandbox.byted_acrawler;
if (!ac || typeof ac.frontierSign !== "function") {
  process.stderr.write("frontierSign missing\n");
  process.exit(3);
}
const out = ac.frontierSign({ "X-MS-STUB": stub });
const sig = (out && (out["X-Bogus"] || out["X-bogus"])) || "";
if (!sig) {
  process.stderr.write("empty X-Bogus\n");
  process.exit(4);
}
process.stdout.write(String(sig));
