// Smoke test for the Node binding: record → resume → replay parity, durable
// effects, typed faults, channels, and hash-chain verification — the same
// scenario the Rust, Python, C ABI, C++, Java, and Go tests run.

'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { Runtime, FaultError, codes, version } = require('../pragmatic');

const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'prag-node-smoke-'));

let oracleCalls = 0;
let effectPerforms = 0;

const rt = new Runtime(
  dir,
  (prompt) => `completion#${oracleCalls++}(${prompt})`,
  Buffer.from('node-smoke-key')
);

const agent = (ctx) => {
  const plan = ctx.oracle('plan the task');
  const findings = [];
  for (let step = 0; step < 3; step++) {
    findings.push(ctx.oracle(`probe ${step}: ${plan}`));
  }
  const published = ctx.effect('publish', `${findings.length} findings`, (arg) => {
    effectPerforms++;
    return `s3://reports/${arg}`;
  });
  return `report(${published})`;
};

// Record.
const report = rt.run('node-research-1', agent);
assert.equal(report.output, 'report(s3://reports/3 findings)');
assert.equal(oracleCalls, 4);
assert.equal(effectPerforms, 1);
assert.equal(report.freshSteps, 4);
assert.equal(report.replayedSteps, 0);
assert.equal(report.chainHead.length, 64);

// Resume: everything comes from the journal — no model calls, no
// re-performed effect.
const resumed = rt.resume('node-research-1', agent);
assert.equal(oracleCalls, 4, 'resume re-sampled');
assert.equal(effectPerforms, 1, 'resume re-performed the effect');
assert.equal(resumed.freshSteps, 0);

// Replay: bit-for-bit trace parity (T1), model never consulted.
const audit = rt.replay('node-research-1', agent);
assert.equal(oracleCalls, 4, 'replay hit the model');
assert.deepEqual(audit.trace, report.trace);

// The chain verifies end to end.
assert.equal(rt.verify('node-research-1'), true);

// Channels.
rt.send('node-approval-1', 'approvals', 'approved');
const approval = rt.run('node-approval-1', (ctx) => ctx.recv('approvals'));
assert.equal(approval.output, 'approved');

// Typed faults surface with the right code.
assert.throws(
  () =>
    rt.run('node-fault-1', (ctx) => {
      ctx.contract(false, 'must not continue');
      return 'unreachable';
    }),
  (e) => e instanceof FaultError && e.code === codes.ERR_CONTRACT && /must not continue/.test(e.message)
);

// A JS exception from an agent is caught at the boundary and rethrown.
assert.throws(
  () =>
    rt.run('node-throw-1', () => {
      throw new Error('agent exploded');
    }),
  /agent exploded/
);

rt.close();
console.log(`node smoke: OK (runtime v${version()})`);
fs.rmSync(dir, { recursive: true, force: true });
