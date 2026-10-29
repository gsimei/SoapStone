import { useEffect, useRef } from "react";
import { basicSetup } from "codemirror";
import { xml } from "@codemirror/lang-xml";
import { Compartment, EditorState, Prec, type Extension } from "@codemirror/state";
import { Decoration, EditorView, MatchDecorator, ViewPlugin, keymap, hoverTooltip, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
import { oneDark } from "@codemirror/theme-one-dark";
import { VARIABLE_RE, type VariableInfo } from "./logic";

const darkMode = window.matchMedia("(prefers-color-scheme: dark)");
const sizing = EditorView.theme({ "&": { height: "100%" }, ".cm-scroller": { fontFamily: "var(--font-mono)", fontSize: "12.5px" } });

/** Marks `{{name}}` as `.cm-var` when defined, `.cm-var-missing` otherwise. */
function variableMarks(defined: Set<string>): Extension {
  const decorator = new MatchDecorator({
    regexp: new RegExp(VARIABLE_RE.source, "g"),
    decoration: (m) => Decoration.mark({ class: defined.has(m[1]) ? "cm-var" : "cm-var-missing" }),
  });
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = decorator.createDeco(view);
      }
      update(u: ViewUpdate) {
        this.decorations = decorator.updateDeco(u, this.decorations);
      }
    },
    { decorations: (v) => v.decorations },
  );
}

export function variableCompletion(variables: VariableInfo[]) {
  return (context: CompletionContext) => {
    const match = context.matchBefore(/\{\{ *[A-Za-z_\w.-]*/);
    if (!match) return null;
    const after = context.state.sliceDoc(context.pos, context.pos + 2);
    return {
      from: match.from, to: context.pos + (after === "}}" ? 2 : 0),
      options: variables.map((v) => ({ label: `{{${v.name}}}`, type: "variable", detail: v.source,
        info: v.secret ? "Secret value (hidden)" : v.value, apply: `{{${v.name}}}` })),
      validFor: /^\{\{ *[\w.-]*$/,
    };
  };
}

function variableTools(variables: VariableInfo[]): Extension {
  return [
    variableMarks(new Set(variables.map((v) => v.name))),
    autocompletion({ override: [variableCompletion(variables)] }),
    hoverTooltip((view, pos) => {
      const line = view.state.doc.lineAt(pos);
      for (const match of line.text.matchAll(new RegExp(VARIABLE_RE.source, "g"))) {
        const start = line.from + match.index;
        if (pos < start || pos > start + match[0].length) continue;
        const variable = variables.find((v) => v.name === match[1]);
        return { pos: start, end: start + match[0].length, create: () => {
          const dom = document.createElement("div");
          dom.className = "variable-tooltip";
          dom.textContent = variable ? `${variable.name} · ${variable.source}\n${variable.secret ? "•••••• (secret)" : variable.value}` : `Undefined variable: ${match[1]}. Add it in Manage environments.`;
          return { dom };
        } };
      }
      return null;
    }),
  ];
}

export function XmlEditor(props: {
  /** A new key replaces the document and its undo history. */
  docKey: string | number;
  value: string;
  onChange?(value: string): void;
  readOnly?: boolean;
  definedVars?: Set<string>;
  variables?: VariableInfo[];
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const onChange = useRef(props.onChange);
  onChange.current = props.onChange;
  const theme = useRef(new Compartment());
  const vars = useRef(new Compartment());

  const state = () =>
    EditorState.create({
      doc: props.value,
      extensions: [
        basicSetup,
        xml(),
        sizing,
        EditorState.readOnly.of(!!props.readOnly),
        theme.current.of(darkMode.matches ? oneDark : []),
        vars.current.of(props.variables ? variableTools(props.variables) : props.definedVars ? variableMarks(props.definedVars) : []),
        // Keeps basicSetup from inserting a line; the window-level handler sends.
        Prec.highest(keymap.of([{ key: "Mod-Enter", run: () => true }])),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onChange.current?.(u.state.doc.toString());
        }),
      ],
    });

  useEffect(() => {
    view.current = new EditorView({ parent: host.current!, state: state() });
    return () => { view.current?.destroy(); view.current = null; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only a new docKey replaces the document
  }, [props.docKey]);

  // Apply toolbar edits as a transaction, keeping the cursor and undo history.
  useEffect(() => {
    const editor = view.current;
    if (editor && editor.state.doc.toString() !== props.value) {
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: props.value } });
    }
  }, [props.value]);

  useEffect(() => {
    const follow = () => view.current?.dispatch({ effects: theme.current.reconfigure(darkMode.matches ? oneDark : []) });
    darkMode.addEventListener("change", follow);
    return () => darkMode.removeEventListener("change", follow);
  }, []);

  useEffect(() => {
    view.current?.dispatch({ effects: vars.current.reconfigure(props.variables ? variableTools(props.variables) : props.definedVars ? variableMarks(props.definedVars) : []) });
  }, [props.definedVars, props.variables]);

  return <div className="xml-editor" ref={host} />;
}
