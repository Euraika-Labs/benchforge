const { sanitizeFilename } = require('./upload');

test('removes path separators', () => {
  expect(sanitizeFilename('../secret.txt')).toBe('.._secret.txt');
  expect(sanitizeFilename('nested/path/file.js')).toBe('nested_path_file.js');
});

test('rejects empty filenames', () => {
  expect(() => sanitizeFilename('')).toThrow();
  expect(() => sanitizeFilename('   ')).toThrow();
});

test('replaces control characters', () => {
  expect(sanitizeFilename('hello\nworld.txt')).toBe('hello_world.txt');
});

test('keeps safe extension', () => {
  expect(sanitizeFilename('report.pdf')).toBe('report.pdf');
});
