import { useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";

/** Two panes side by side with a draggable divider; the first pane's share of the width is kept in localStorage. */
export function Split(props: { storageKey: string; initial: number; min: number; children: [ReactNode, ReactNode] }) {
  const key = `soapstone.split.${props.storageKey}`;
  const clamp = (v: number) => Math.min(1 - props.min, Math.max(props.min, v));
  const [share, setShare] = useState(() => {
    const saved = Number(localStorage.getItem(key));
    return saved > 0 && saved < 1 ? clamp(saved) : props.initial;
  });
  const box = useRef<HTMLDivElement>(null);
  const save = (v: number) => localStorage.setItem(key, String(v));

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId) || !box.current) return;
    const r = box.current.getBoundingClientRect();
    setShare(clamp((e.clientX - r.left) / r.width));
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.key === "ArrowLeft" ? -0.02 : e.key === "ArrowRight" ? 0.02 : 0;
    if (!step) return;
    const next = clamp(share + step);
    setShare(next);
    save(next);
  };

  return (
    <div className="split" ref={box}>
      <div className="pane" style={{ flexBasis: `${share * 100}%` }}>
        {props.children[0]}
      </div>
      <div
        className="divider"
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={Math.round(share * 100)}
        tabIndex={0}
        onPointerDown={(e) => e.currentTarget.setPointerCapture(e.pointerId)}
        onPointerMove={onPointerMove}
        onPointerUp={(e) => {
          e.currentTarget.releasePointerCapture(e.pointerId);
          save(share);
        }}
        onKeyDown={onKeyDown}
      />
      <div className="pane grow">{props.children[1]}</div>
    </div>
  );
}
