/** A small interlinked fixture cairn used by the mock and dev. */
export const FIXTURE_NOTES: Record<string, string> = {
  "index.md": "# Index\n\nStart at [[ideas]] or the [[todo]] list.",
  "ideas.md": "# Ideas\n\nA thought that links back to [[index]].",
  "todo.md":
    "---\ntags: [rust]\n---\n# Todo\n\n- review [[ideas]]\n- nothing links here yet",
  "projects/demo.md":
    "---\ntags: [rust, ideas]\n---\n# Demo\n\nA standalone nested note.",
  "kitchensink.md": `# Kitchen sink

A paragraph with **bold** text and a [[ideas]] link.

- first bullet
- second bullet

- [ ] open task
- [x] done task

> a quoted line

---

\`\`\`js
const x = 1;
\`\`\`

| A | B |
|---|---|
| 1 | 2 |

![logo](img/logo.png)
`,
};
