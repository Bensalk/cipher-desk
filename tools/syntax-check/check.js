// Syntax-only check of Dart (.dart) and Kotlin (.kt) files using tree-sitter grammars.
// Usage: node check.js <file>...   (exit code 1 if any file has a syntax error)
// It finds missing or extra brackets and similar mistakes. It does NOT check
// types or imports: `flutter analyze` (run in CI) is still required.
'use strict';
const fs = require('fs');
const path = require('path');
const { Parser, Language } = require('web-tree-sitter');

(async () => {
  await Parser.init();
  const dir = path.join(path.dirname(require.resolve('tree-sitter-wasms/package.json', { paths: [__dirname] })), 'out');
  const grammars = { '.dart': 'dart', '.kt': 'kotlin' };
  const parsers = {};
  let bad = 0;
  for (const file of process.argv.slice(2)) {
    const lang = grammars[path.extname(file)];
    if (!lang) {
      console.log(`${file}: not checked (only .dart and .kt)`);
      continue;
    }
    if (!parsers[lang]) {
      parsers[lang] = new Parser();
      parsers[lang].setLanguage(await Language.load(path.join(dir, `tree-sitter-${lang}.wasm`)));
    }
    const tree = parsers[lang].parse(fs.readFileSync(file, 'utf8'));
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
  console.log(bad ? `syntax-check: ${bad} file(s) with errors` : 'syntax-check: OK');
  process.exit(bad ? 1 : 0);
})().catch((e) => {
  console.error('syntax-check: could not run:', e.message);
  process.exit(2);
});
