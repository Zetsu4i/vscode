import { monaco } from "../monaco";

/**
 * Monaco's bundled basic-languages ship decent defaults, but region folding
 * markers, a few missing languages, and markdown list continuation are worth
 * registering explicitly. All registrations merge with existing configs and
 * are idempotent.
 */

let registered = false;

export function registerLanguageConfigs(): void {
  if (registered) return;
  registered = true;

  // --- proto: not bundled by Monaco at all — give it a C-like setup --------
  monaco.languages.setLanguageConfiguration("proto", {
    comments: {
      lineComment: "//",
      blockComment: ["/*", "*/"],
    },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
      ["<", ">"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"', notIn: ["string"] },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
    folding: {
      markers: {
        start: /^\s*\/\/\s*#?region\b/,
        end: /^\s*\/\/\s*#?endregion\b/,
      },
    },
  });

  // --- region folding markers for line-comment families ---------------------
  const lineCommentFolds: [string, RegExp, RegExp][] = [
    // C-family: // region / // endregion (and the #region spelling)
    ["rust", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["go", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["c", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["cpp", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["csharp", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["java", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["javascript", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["typescript", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["kotlin", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["swift", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["dart", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["php", /^\s*\/\/\s*#?region\b/, /^\s*\/\/\s*#?endregion\b/],
    ["shell", /^\s*#\s*region\b/, /^\s*#\s*endregion\b/],
    ["python", /^\s*#\s*region\b/, /^\s*#\s*endregion\b/],
    ["perl", /^\s*#\s*region\b/, /^\s*#\s*endregion\b/],
    ["yaml", /^\s*#\s*region\b/, /^\s*#\s*endregion\b/],
    ["hcl", /^\s*#\s*region\b/, /^\s*#\s*endregion\b/],
    ["ruby", /^\s*#\s*region\b/, /^\s*#\s*endregion\b/],
    // INI family (covers toml via our mapping)
    ["ini", /^\s*[;#]\s*region\b/, /^\s*[;#]\s*endregion\b/],
  ];

  for (const [lang, start, end] of lineCommentFolds) {
    monaco.languages.setLanguageConfiguration(lang, {
      folding: { markers: { start, end } },
    });
  }

  // CSS-family block-comment folds
  const blockCommentFolds: [string, RegExp, RegExp][] = [
    ["css", /^\s*\/\*\s*#?region\b/, /^\s*.*\*\/\s*$/],
    ["scss", /^\s*\/\*\s*#?region\b/, /^\s*.*\*\/\s*$/],
    ["less", /^\s*\/\*\s*#?region\b/, /^\s*.*\*\/\s*$/],
  ];
  for (const [lang, start, end] of blockCommentFolds) {
    monaco.languages.setLanguageConfiguration(lang, {
      folding: { markers: { start, end } },
    });
  }

  // --- markdown: continue lists on Enter ------------------------------------
  monaco.languages.setLanguageConfiguration("markdown", {
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    surroundingPairs: [
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: "*", close: "*" },
      { open: "_", close: "_" },
      { open: "`", close: "`" },
    ],
    onEnterRules: [
      {
        // "- item" → Enter → "- "
        beforeText: /^\s*-\s+.*/,
        action: { indentAction: monaco.languages.IndentAction.None, appendText: "- " },
      },
      {
        // "* item" → Enter → "* "
        beforeText: /^\s*\*\s+.*/,
        action: { indentAction: monaco.languages.IndentAction.None, appendText: "* " },
      },
      {
        // "> quote" → Enter → "> "
        beforeText: /^\s*>\s+.*/,
        action: { indentAction: monaco.languages.IndentAction.None, appendText: "> " },
      },
    ],
  });
}
