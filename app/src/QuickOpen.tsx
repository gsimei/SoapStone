import { useEffect, useMemo, useRef, useState } from "react";
import type { ServiceNode } from "./api";
import { matches, plain, quickOpenItems } from "./logic";

export function QuickOpen(props: { tree: ServiceNode[]; onOpen(requestId: number): void; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const items = useMemo(() => quickOpenItems(props.tree), [props.tree]);
  const shown = items.filter((i) => matches(i.label, query)).slice(0, 50);

  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  useEffect(() => {
    list.current?.children[index]?.scrollIntoView({ block: "nearest" });
  }, [index]);

  return (
    <dialog
      ref={dialog}
      className="dialog quick-open"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
    >
      <input {...plain}
        autoFocus
        aria-label="Go to request"
        placeholder="Go to request…"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setIndex(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") setIndex((i) => Math.min(i + 1, shown.length - 1));
          else if (e.key === "ArrowUp") setIndex((i) => Math.max(i - 1, 0));
          else if (e.key === "Enter" && shown[index]) props.onOpen(shown[index].requestId);
          else return;
          e.preventDefault();
        }}
      />
      <ul ref={list} role="listbox" aria-label="Requests">
        {shown.map((item, i) => (
          <li key={item.requestId} role="option" aria-selected={i === index}>
            <button tabIndex={-1} onMouseMove={() => setIndex(i)} onClick={() => props.onOpen(item.requestId)}>
              {item.label}
            </button>
          </li>
        ))}
        {shown.length === 0 && <li className="muted empty-item">No matching requests.</li>}
      </ul>
    </dialog>
  );
}
