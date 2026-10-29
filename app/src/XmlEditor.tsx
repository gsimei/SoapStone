import { useEffect, useRef } from "react";
import { basicSetup } from "codemirror";
import { xml } from "@codemirror/lang-xml";
import { Compartment, EditorState, Prec, type Extension } from "@codemirror/state";
import { Decoration, EditorView, MatchDecorator, ViewPlugin, keymap, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { oneDark } from "@codemirror/theme-one-dark";
import { VARIABLE_RE } from "./logic";

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

export function XmlEditor(props: {
  /** A new key replaces the document and its undo history. */
  docKey: string | number;
  value: string;
  onChange?(value: string): void;
  readOnly?: boolean;
  definedVars?: Set<string>;
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
        vars.current.of(props.definedVars ? variableMarks(props.definedVars) : []),
        // Keeps basicSetup from inserting a line; the window-level handler sends.
        Prec.highest(keymap.of([{ key: "Mod-Enter", run: () => true }])),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onChange.current?.(u.state.doc.toString());
        }),
      ],
    });

  useEffect(() => {
    if (view.current) view.current.setState(state());
    else view.current = new EditorView({ parent: host.current!, state: state() });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only a new docKey replaces the document
  }, [props.docKey]);

  useEffect(() => () => view.current?.destroy(), []);

  useEffect(() => {
    const follow = () => view.current?.dispatch({ effects: theme.current.reconfigure(darkMode.matches ? oneDark : []) });
    darkMode.addEventListener("change", follow);
    return () => darkMode.removeEventListener("change", follow);
  }, []);

  useEffect(() => {
    view.current?.dispatch({ effects: vars.current.reconfigure(props.definedVars ? variableMarks(props.definedVars) : []) });
  }, [props.definedVars]);

  return <div className="xml-editor" ref={host} />;
}
