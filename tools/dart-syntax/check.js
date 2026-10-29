// Syntax-only check of Dart files using the tree-sitter Dart grammar.
// Usage: node check.js <file.dart>...   (exit code 1 if any file has a syntax error)
// It finds missing or extra brackets and similar mistakes. It does NOT check
// types or imports: `flutter analyze` (run in CI) is still required.
'use strict';
const fs = require('fs');
const path = require('path');
const { Parser, Language } = require('web-tree-sitter');

(async () => {
  await Parser.init();
  const wasm = path.join(path.dirname(require.resolve('tree-sitter-wasms/package.json', { paths: [__dirname] })), 'out', 'tree-sitter-dart.wasm');
  const parser = new Parser();
  parser.setLanguage(await Language.load(wasm));
  let bad = 0;
  for (const file of process.argv.slice(2)) {
    const tree = parser.parse(fs.readFileSync(file, 'utf8'));
    if (!tree.rootNode.hasError) continue;
    const errors = [];
    const walk = (n) => {
      if (n.type === 'ERROR' || n.isMissing) {
        errors.push(`${file}:${n.startPosition.row + 1}:${n.startPosition.column + 1}: ${n.isMissing ? 'missing ' + n.type : 'syntax error'}`);
      }
      for (let i = 0; i < n.childCount; i++) walk(n.child(i));
    };
    walk(tree.rootNode);
    console.log(errors.slice(0, 5).join('\n'));
    bad++;
  }
  console.log(bad ? `dart-syntax: ${bad} file(s) with errors` : 'dart-syntax: OK');
  process.exit(bad ? 1 : 0);
})().catch((e) => {
  console.error('dart-syntax: could not run:', e.message);
  process.exit(2);
});
