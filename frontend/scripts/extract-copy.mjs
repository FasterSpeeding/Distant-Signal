// Prints every user-visible string candidate in the app's TypeScript as
// JSON lines, for scripts/check-copy.py (the copy guardrail in CI).
//
//   node scripts/extract-copy.mjs [file ...]
//
// With no arguments it walks app/, components/ and lib/, skipping tests.
// Each line is one string:
//   file     path relative to frontend/
//   line     1-based line of the string
//   kind     "jsx" (JSX text), "attr" (a JSX attribute's literal),
//            "string" or "template" (any other literal), or "heading"
//            (a Title's or SectionTitle's whole text, as well as its parts)
//   text     the text, whitespace collapsed; a template's `${...}` holes
//            become "{}"
//   attr     the JSX attribute's name, for kind "attr"
//   prop     the object key a literal is the value of (`title: '...'`)
//   decl     the variable a literal initialises (`const X = '...'`)
//   element  the nearest enclosing JSX element's tag name
//   order    that element's literal `order` prop, if any
// Uses the TypeScript compiler the frontend already depends on, so a
// string is found exactly where the parser puts it.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';

const ROOT = new URL('..', import.meta.url).pathname;
const DIRS = ['app', 'components', 'lib'];

/** @param {string} dir @returns {string[]} */
function walk(dir) {
  /** @type {string[]} */
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      out.push(...walk(path));
    } else if (/\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) && !name.endsWith('.d.ts')) {
      out.push(path);
    }
  }
  return out;
}

/** @type {Record<string, string>} */
const ENTITIES = { amp: '&', apos: "'", quot: '"', lt: '<', gt: '>', nbsp: ' ', mdash: '—', ndash: '–', hellip: '…' };

/** @param {string} text */
function collapse(text) {
  return text.replace(/\s+/g, ' ').trim();
}

/** @param {string} text */
function decodeEntities(text) {
  return text.replace(/&([a-z]+);/g, (whole, name) => ENTITIES[String(name)] ?? whole);
}

/** @param {ts.JsxOpeningLikeElement} element */
function literalOrder(element) {
  for (const attribute of element.attributes.properties) {
    if (!ts.isJsxAttribute(attribute) || attribute.name.getText() !== 'order') continue;
    const init = attribute.initializer;
    if (init && ts.isJsxExpression(init) && init.expression && ts.isNumericLiteral(init.expression)) {
      return Number(init.expression.text);
    }
  }
  return undefined;
}

/** Nodes a literal's value passes straight through on its way to an
 * attribute, property, variable or JSX child. Anything else (a call's
 * argument, say) ends the walk: the literal is no longer the value itself.
 * @param {ts.Node} node */
function passesThrough(node) {
  return (
    ts.isParenthesizedExpression(node) ||
    ts.isConditionalExpression(node) ||
    ts.isBinaryExpression(node) ||
    ts.isJsxExpression(node) ||
    ts.isAsExpression(node) ||
    ts.isSatisfiesExpression(node) ||
    ts.isJsxAttribute(node) ||
    ts.isJsxAttributes(node) ||
    ts.isPropertyAssignment(node) ||
    ts.isObjectLiteralExpression(node) ||
    ts.isArrayLiteralExpression(node)
  );
}

/** @param {ts.Node} node */
function context(node) {
  /** @type {{attr?: string, prop?: string, decl?: string, element?: string, order?: number}} */
  const found = {};
  let child = node;
  // The root SourceFile has no parent at run time, whatever the types say.
  let current = /** @type {ts.Node | undefined} */ (node.parent);
  while (current !== undefined) {
    if (ts.isJsxAttribute(current)) found.attr ??= current.name.getText();
    if (ts.isPropertyAssignment(current) && current.initializer === child) {
      found.prop ??= current.name.getText().replace(/^['"]|['"]$/g, '');
    }
    if (ts.isVariableDeclaration(current)) {
      if (current.initializer === child) found.decl = current.name.getText();
      break;
    }
    const opening = ts.isJsxElement(current)
      ? current.openingElement
      : ts.isJsxOpeningElement(current) || ts.isJsxSelfClosingElement(current)
        ? current
        : undefined;
    if (opening !== undefined) {
      found.element = opening.tagName.getText();
      const order = literalOrder(opening);
      if (order !== undefined) found.order = order;
      break;
    }
    if (!passesThrough(current)) break;
    child = current;
    current = current.parent;
  }
  return found;
}

/** A heading's whole text: its JSX text and literal children, with any
 * other expression as "{}".
 * @param {ts.JsxElement} element @param {ts.SourceFile} file */
function headingText(element, file) {
  return collapse(
    element.children
      .map((child) => {
        if (ts.isJsxText(child)) return decodeEntities(child.getText(file));
        if (ts.isJsxExpression(child) && child.expression && ts.isStringLiteral(child.expression)) {
          return child.expression.text;
        }
        if (ts.isJsxExpression(child) && child.expression === undefined) return '';
        return '{}';
      })
      .join(''),
  );
}

const HEADINGS = new Set(['Title', 'SectionTitle']);

/** @param {string} path */
function extract(path) {
  const source = readFileSync(path, 'utf8');
  const file = ts.createSourceFile(
    path,
    source,
    ts.ScriptTarget.Latest,
    true,
    path.endsWith('x') ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const rel = relative(ROOT, path);
  /** @type {object[]} */
  const out = [];
  /** @param {ts.Node} node @param {string} kind @param {string} text */
  const push = (node, kind, text) => {
    if (!/[A-Za-z]{2}/.test(text)) return;
    const { line } = file.getLineAndCharacterOfPosition(node.getStart(file));
    out.push({ file: rel, line: line + 1, kind, text, ...context(node) });
  };
  /** @param {ts.Node} node */
  const visit = (node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node) || ts.isLiteralTypeNode(node)) return;
    if (ts.isJsxElement(node) && HEADINGS.has(node.openingElement.tagName.getText())) {
      const { line } = file.getLineAndCharacterOfPosition(node.getStart(file));
      const order = literalOrder(node.openingElement);
      out.push({
        file: rel,
        line: line + 1,
        kind: 'heading',
        text: headingText(node, file),
        element: node.openingElement.tagName.getText(),
        ...(order === undefined ? {} : { order }),
      });
    }
    if (ts.isJsxText(node)) {
      push(node, 'jsx', collapse(decodeEntities(node.getText(file))));
    } else if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      push(node, ts.isJsxAttribute(node.parent) ? 'attr' : 'string', collapse(node.text));
    } else if (ts.isTemplateExpression(node)) {
      const parts = [node.head.text, ...node.templateSpans.map((span) => span.literal.text)];
      push(node, 'template', collapse(parts.join('{}')));
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return out;
}

const files = process.argv.length > 2 ? process.argv.slice(2) : DIRS.flatMap((dir) => walk(join(ROOT, dir)));
for (const path of files) {
  for (const record of extract(path)) {
    process.stdout.write(`${JSON.stringify(record)}\n`);
  }
}
