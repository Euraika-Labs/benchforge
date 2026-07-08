const test = require('node:test');
const assert = require('node:assert/strict');
const { retryDelay } = require('./retry');

test('uses exponential backoff from a 100 ms base', () => {
  assert.equal(retryDelay(0), 100);
  assert.equal(retryDelay(1), 200);
  assert.equal(retryDelay(2), 400);
  assert.equal(retryDelay(3), 800);
});

test('caps delay at 1600 ms', () => {
  assert.equal(retryDelay(4), 1600);
  assert.equal(retryDelay(8), 1600);
});

test('rejects invalid attempts', () => {
  assert.throws(() => retryDelay(-1), TypeError);
  assert.throws(() => retryDelay(1.5), TypeError);
});
