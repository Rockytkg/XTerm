import assert from "node:assert/strict";
import test from "node:test";
import { history, redo, undo } from "@codemirror/commands";
import { EditorState } from "@codemirror/state";

function createCommandTarget(content) {
  let state = EditorState.create({ doc: content, extensions: [history()] });
  return {
    get state() {
      return state;
    },
    dispatch(transaction) {
      state = transaction.state;
    },
  };
}

test("editor history can undo and redo a formatting replacement", () => {
  const target = createCommandTarget("const value={answer:42}");
  target.dispatch(
    target.state.update({
      changes: {
        from: 0,
        to: target.state.doc.length,
        insert: "const value = { answer: 42 };\n",
      },
    }),
  );

  assert.equal(undo(target), true);
  assert.equal(target.state.doc.toString(), "const value={answer:42}");
  assert.equal(redo(target), true);
  assert.equal(target.state.doc.toString(), "const value = { answer: 42 };\n");
});
